//! Explicit admitted retirement over annex bytes. No native payload receipt or
//! second journal; source/job exclusion and outcome capacity use the index.
use super::*;
use crate::annex::{
    process::Enrollment,
    worker::{self, Worker},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    path::PathBuf,
};
use tig_core::retirement::review::{Loss, decision::Target};
use tig_store::{
    Key as ControlKey,
    metadata::{self, MetadataStore},
    retirement_intent::{self, ObservedIntent},
    source_claims::{
        self, SourceJournal,
        protection::effects::{Admission, Outcome, Request},
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataBinding {
    pub root: PathBuf,
    pub minimum: metadata::Checkpoint,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentBinding {
    pub root: PathBuf,
    pub minimum: source_claims::Checkpoint,
    pub owner: ControlKey,
    pub subject: Digest,
    pub operation: Digest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Custody {
    /// Owner-admitted physical failure-domain mapping, not inferred from paths.
    pub domains: BTreeMap<Digest, Digest>,
    pub minimum_domains: u16,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<super::policy::Binding>,
    pub target: Enrollment,
    pub metadata: MetadataBinding,
    pub intent: IntentBinding,
    pub request: Request,
    pub custody: Custody,
    pub survivors: Vec<Enrollment>,
    pub verification_bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageProfile {
    pub base: Profile,
    pub review_input: PathBuf,
    pub consent: tig_core::retirement::review::decision::Consent,
    pub operation: Digest,
}
/// Explicit owner-admitted staging using the original intent publisher. The
/// consent lists exact targets and any intentional-loss objects; no label or
/// elapsed time manufactures consent. Returned profiles are bounded asset-side
/// work references suitable for saving outside authored Git data.
pub fn stage(input: &StageProfile) -> io::Result<Vec<Profile>> {
    stage_bound(input, false)
}
pub(super) fn stage_bound(input: &StageProfile, automatic: bool) -> io::Result<Vec<Profile>> {
    stage_bound_hook(input, automatic, |_| Ok(()))
}
/// Metadata staging boundaries for callers that test abrupt process exit.
/// Observing a boundary grants no permission to execute a payload effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagingPoint {
    IntentPrepared,
    IntentAdopted,
}
pub(super) fn stage_bound_hook(
    input: &StageProfile,
    automatic: bool,
    mut hook: impl FnMut(StagingPoint) -> io::Result<()>,
) -> io::Result<Vec<Profile>> {
    use std::io::Read as _;
    use tig_core::retirement::review::{
        Review,
        decision::{Approval, Decision},
    };
    let base = &input.base;
    if automatic != base.policy.is_some() {
        return Err(invalid());
    }
    if !input.review_input.is_absolute()
        || !base.metadata.root.is_absolute()
        || !base.intent.root.is_absolute()
    {
        return Err(invalid());
    }
    let binding = base.target.control.as_ref().ok_or_else(invalid)?;
    let mut index = Index::open(&binding.root, &binding.minimum).map_err(|_| invalid())?;
    if index.identity() != &binding.index
        || !index.supports_effect_fences()
        || index.enrollment_profile().owner != base.intent.owner
        || index.enrollment_profile().subject != base.intent.subject
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    File::open(&input.review_input)?
        .take(65_537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err(invalid());
    }
    let review = Review::from_json(&bytes).map_err(|_| invalid())?;
    if !index
        .enrollment_profile()
        .policies
        .contains(review.policy_identity())
    {
        return Err(invalid());
    }
    let expected = index.effect_frame().map_err(|_| invalid())?;
    let evidence = approval_evidence(index.identity(), &expected, &base.custody)?;
    let mut nodes =
        MetadataStore::open(&base.metadata.root, &base.metadata.minimum).map_err(|_| invalid())?;
    let mut journal =
        SourceJournal::open(&base.intent.root, &base.intent.minimum).map_err(|_| invalid())?;
    let owner = retirement_intent::Enrollment {
        owner: base.intent.owner.clone(),
        subject: base.intent.subject.clone(),
    };
    let evidence = super::policy::evidence(base, &nodes, &review, evidence)?;
    let decision = Decision::authorize(
        &review,
        Approval {
            namespace: journal.identity().clone(),
            operation: input.operation.clone(),
            owner: base.intent.subject.clone(),
            evidence: evidence.clone(),
        },
        input.consent.clone(),
    )
    .map_err(|_| invalid())?;
    if decision.consent().remove_copies.len() > 128
        || base.custody.domains.keys().cloned().collect::<Vec<_>>()
            != index.enrollment_profile().stores
    {
        return Err(invalid());
    }
    let mut providers = BTreeMap::new();
    for provider in std::iter::once(&base.target).chain(&base.survivors) {
        let control = provider.control.as_ref().ok_or_else(invalid)?;
        if control.index != binding.index
            || control.root != binding.root
            || !index.enrollment_profile().stores.contains(&control.store)
            || providers
                .insert(control.store.clone(), provider.clone())
                .is_some()
        {
            return Err(invalid());
        }
    }
    for target in &decision.consent().remove_copies {
        if !providers.contains_key(&target.store) {
            return Err(invalid());
        }
        let reviewed = review
            .objects()
            .iter()
            .find(|v| v.object() == &target.object)
            .ok_or_else(invalid)?;
        if let Loss::ObservedSurvivors(stores) = reviewed.loss()
            && stores.iter().any(|s| !providers.contains_key(s))
        {
            return Err(invalid());
        }
    }
    let mut pending = None;
    let existing =
        match retirement_intent::inspect_retained(&mut journal, &nodes, &owner, &input.operation) {
            Ok(existing) => {
                if existing.decision().digest().map_err(|_| invalid())?
                    != decision.digest().map_err(|_| invalid())?
                {
                    return Err(invalid());
                }
                true
            }
            Err(retirement_intent::Error::Missing) => false,
            Err(retirement_intent::Error::Pending) => {
                let prepared = retirement_intent::resume_pending(
                    &mut journal,
                    &mut nodes,
                    &owner,
                    &input.operation,
                )
                .map_err(|_| invalid())?;
                if prepared
                    .decision(&nodes)
                    .map_err(|_| invalid())?
                    .digest()
                    .map_err(|_| invalid())?
                    != decision.digest().map_err(|_| invalid())?
                {
                    return Err(invalid());
                }
                pending = Some(prepared);
                false
            }
            Err(_) => return Err(invalid()),
        };
    let policy_binding = super::policy::reserve(base, &mut index, &mut nodes, &review, &decision)?;
    if !existing {
        let prepared = if let Some(prepared) = pending {
            prepared
        } else {
            let candidate =
                retirement_intent::stage(&mut journal, &mut nodes, &owner, &review, &decision)
                    .map_err(|_| invalid())?;
            let ready = retirement_intent::finish(&mut journal, &mut nodes, candidate)
                .map_err(|_| invalid())?;
            let prepare = ControlKey::parse(&format!("annex-stage-{}", input.operation.as_str()))
                .map_err(|_| invalid())?;
            retirement_intent::prepare(&mut journal, &nodes, ready, prepare)
                .map_err(|_| invalid())?
        };
        hook(StagingPoint::IntentPrepared)?;
        let adopt = ControlKey::parse(&format!("annex-adopt-{}", input.operation.as_str()))
            .map_err(|_| invalid())?;
        retirement_intent::adopt(&mut journal, &nodes, &prepared, adopt).map_err(|_| invalid())?;
        hook(StagingPoint::IntentAdopted)?;
    }
    let mut profiles = Vec::new();
    for target in &decision.consent().remove_copies {
        let mut profile = base.clone();
        profile.policy = policy_binding.clone();
        profile.target = providers.get(&target.store).ok_or_else(invalid)?.clone();
        profile.intent.operation = input.operation.clone();
        profile.intent.minimum = journal.checkpoint();
        profile.metadata.minimum = nodes.checkpoint();
        profile.request = Request {
            operation: operation(&input.operation, &target.object, &target.store)?,
            object: target.object.clone(),
            store: target.store.clone(),
            context: action_context(
                &decision.digest().map_err(|_| invalid())?,
                &evidence,
                &target.store,
            )?,
            expected: expected.clone(),
        };
        let object = review
            .objects()
            .iter()
            .find(|v| v.object() == &target.object)
            .ok_or_else(invalid)?;
        profile.survivors = match object.loss() {
            Loss::ObservedSurvivors(stores) => stores
                .iter()
                .map(|s| providers.get(s).cloned().ok_or_else(invalid))
                .collect::<io::Result<Vec<_>>>()?,
            _ => vec![],
        };
        profiles.push(profile);
    }
    Ok(profiles)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Removed,
    NoEffect,
    HistoricalRemoved,
    HistoricalNoEffect,
    Uncertain,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub status: Status,
    pub object: Object,
    pub store: Digest,
    pub reclaimed_bytes: Option<u64>,
}
fn report(profile: &Profile, status: Status) -> Report {
    Report {
        status,
        object: profile.request.object.clone(),
        store: profile.request.store.clone(),
        reclaimed_bytes: None,
    }
}
fn encoded(value: &impl Serialize) -> io::Result<Vec<u8>> {
    tig_core::identity::canonical_json(&serde_json::to_value(value).map_err(|_| invalid())?)
        .map_err(|_| invalid())
}
pub fn approval_evidence(
    index: &Digest,
    expected: &source_claims::Selection,
    custody: &Custody,
) -> io::Result<Digest> {
    if custody.domains.is_empty() || custody.domains.len() > 16 || custody.minimum_domains > 16 {
        return Err(invalid());
    }
    Ok(Digest::blob(&encoded(&(
        "tig.annex-retirement-approval.design-1",
        index,
        expected,
        custody,
    ))?))
}
pub fn operation(
    decision_operation: &Digest,
    object: &Object,
    store: &Digest,
) -> io::Result<ControlKey> {
    ControlKey::parse(&format!(
        "annex-{}",
        Digest::blob(&encoded(&(
            "tig.annex-retirement-operation.design-1",
            decision_operation,
            object,
            store
        ))?)
        .as_str()
    ))
    .map_err(|_| invalid())
}
pub fn action_context(decision: &Digest, evidence: &Digest, store: &Digest) -> io::Result<Digest> {
    Ok(Digest::blob(&encoded(&(
        "tig.annex-retirement-action.design-1",
        decision,
        evidence,
        store,
    ))?))
}
struct Controls {
    index: Index,
    nodes: MetadataStore,
    journal: SourceJournal,
    intent: ObservedIntent,
}
fn controls(profile: &Profile) -> io::Result<Controls> {
    if !profile.metadata.root.is_absolute()
        || !profile.intent.root.is_absolute()
        || profile.survivors.len() > 16
        || profile
            .request
            .object
            .bytes
            .checked_mul(profile.survivors.len() as u64 + 2)
            .is_none_or(|n| n > profile.verification_bytes)
    {
        return Err(invalid());
    }
    let binding = profile.target.control.as_ref().ok_or_else(invalid)?;
    let index = Index::open(&binding.root, &binding.minimum).map_err(|_| invalid())?;
    if index.identity() != &binding.index
        || !index.supports_effect_fences()
        || binding.store != profile.request.store
        || index.enrollment_profile().owner != profile.intent.owner
        || index.enrollment_profile().subject != profile.intent.subject
        || profile.custody.domains.keys().cloned().collect::<Vec<_>>()
            != index.enrollment_profile().stores
    {
        return Err(invalid());
    }
    let nodes = MetadataStore::open(&profile.metadata.root, &profile.metadata.minimum)
        .map_err(|_| invalid())?;
    let mut journal = SourceJournal::open(&profile.intent.root, &profile.intent.minimum)
        .map_err(|_| invalid())?;
    let intent = retirement_intent::inspect_retained(
        &mut journal,
        &nodes,
        &retirement_intent::Enrollment {
            owner: profile.intent.owner.clone(),
            subject: profile.intent.subject.clone(),
        },
        &profile.intent.operation,
    )
    .map_err(|_| invalid())?;
    // Native profiled policy authorities are not silently reinterpreted as annex.
    if intent.execution_profile().is_some()
        || intent.decision().consent().remove_copies.len() > 128
        || !index
            .enrollment_profile()
            .policies
            .contains(intent.review().policy_identity())
        || intent.review().owner() != &profile.intent.subject
        || !intent.decision().consent().remove_copies.contains(&Target {
            object: profile.request.object.clone(),
            store: binding.store.clone(),
        })
        || intent
            .decision()
            .consent()
            .release_owned_claims
            .iter()
            .any(|r| r.object == profile.request.object)
    {
        return Err(invalid());
    }
    let evidence = approval_evidence(
        index.identity(),
        &profile.request.expected,
        &profile.custody,
    )?;
    let evidence = super::policy::evidence(profile, &nodes, intent.review(), evidence)?;
    super::policy::verify(profile, &nodes, intent.review(), intent.decision())?;
    if intent.decision().approval().evidence != evidence
        || profile.request.operation
            != operation(
                &profile.intent.operation,
                &profile.request.object,
                &binding.store,
            )?
        || profile.request.context
            != action_context(
                &intent.decision().digest().map_err(|_| invalid())?,
                &evidence,
                &binding.store,
            )?
    {
        return Err(invalid());
    }
    Ok(Controls {
        index,
        nodes,
        journal,
        intent,
    })
}
fn active(profile: &Profile, controls: &mut Controls) -> io::Result<()> {
    retirement_intent::observe(
        &mut controls.journal,
        &controls.nodes,
        &retirement_intent::Enrollment {
            owner: profile.intent.owner.clone(),
            subject: profile.intent.subject.clone(),
        },
        &profile.intent.operation,
    )
    .map_err(|_| invalid())?;
    super::policy::hold(
        profile,
        &mut controls.index,
        &controls.nodes,
        controls.intent.review(),
        controls.intent.decision(),
    )?;
    Ok(())
}
struct BindingHold {
    _directories: Vec<File>,
    _config: File,
    uuid: Uuid,
}
fn bind(runner: &Runner, store: &Digest) -> io::Result<BindingHold> {
    let root = runner.repository();
    let git = if root.join(".git").exists() {
        root.join(".git")
    } else {
        root.to_owned()
    };
    let directories = crate::annex::legacy::hold_directories(&git)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x00200000);
    }
    let config = options.open(git.join("config"))?;
    if !config.metadata()?.is_file() || config.metadata()?.len() > 65_536 {
        return Err(invalid());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if config.metadata()?.file_attributes() & 0x400 != 0 {
            return Err(invalid());
        }
    }
    let information = info(runner, None)?;
    let uuid = uuid(&information)?;
    if uuid.store() != *store
        || information.get("trust").and_then(|v| v.as_str()) == Some("untrusted")
    {
        return Err(invalid());
    }
    Ok(BindingHold {
        _directories: directories,
        _config: config,
        uuid,
    })
}
struct Keepers {
    _bindings: Vec<BindingHold>,
    _copies: Vec<VerifiedCopy>,
}
fn keepers(profile: &Profile, controls: &mut Controls, key: &Key) -> io::Result<(Keepers, bool)> {
    let review = controls
        .intent
        .review()
        .objects()
        .iter()
        .find(|v| v.object() == &profile.request.object)
        .ok_or_else(invalid)?;
    if !review.eligible() || !review.targets().contains(&profile.request.store) {
        return Err(invalid());
    }
    let intentional = controls
        .intent
        .decision()
        .consent()
        .accept_intentional_loss
        .contains(&profile.request.object);
    let expected = match review.loss() {
        Loss::NoObservedSurvivor
            if intentional
                && !controls.intent.review().is_routine_eviction()
                && profile.survivors.is_empty() =>
        {
            return Ok((
                Keepers {
                    _bindings: vec![],
                    _copies: vec![],
                },
                true,
            ));
        }
        Loss::ObservedSurvivors(stores) if !intentional && profile.custody.minimum_domains > 0 => {
            stores
        }
        _ => return Err(invalid()),
    };
    let target_domain = profile
        .custody
        .domains
        .get(&profile.request.store)
        .ok_or_else(invalid)?;
    let target_control = profile.target.control.as_ref().ok_or_else(invalid)?;
    let mut stores = BTreeSet::new();
    let mut domains = BTreeSet::new();
    let mut kept = Keepers {
        _bindings: vec![],
        _copies: vec![],
    };
    for enrollment in &profile.survivors {
        let control = enrollment.control.as_ref().ok_or_else(invalid)?;
        if control.index != target_control.index
            || std::fs::canonicalize(&control.root)? != std::fs::canonicalize(&target_control.root)?
            || control.store == profile.request.store
            || !stores.insert(control.store.clone())
            || !expected.contains(&control.store)
            || controls
                .index
                .inspect(&mut controls.nodes, &key.object(), &control.store)
                .map_err(|_| invalid())?
                .protected_by
                .is_empty()
        {
            return Err(invalid());
        }
        let runner = Runner::new(enrollment.clone())?;
        kept._bindings.push(bind(&runner, &control.store)?);
        kept._copies
            .push(hold_copy(&runner, key, profile.verification_bytes)?.ok_or_else(invalid)?);
        let domain = profile
            .custody
            .domains
            .get(&control.store)
            .ok_or_else(invalid)?;
        if domain != target_domain {
            domains.insert(domain.clone());
        }
    }
    if stores != *expected || domains.len() < profile.custody.minimum_domains as usize {
        return Err(invalid());
    }
    Ok((kept, false))
}
fn observation(profile: &Profile, kind: &str) -> io::Result<Digest> {
    Ok(Digest::blob(&encoded(&(
        "tig.annex-effect-observation.design-1",
        &profile.request,
        kind,
    ))?))
}
fn historical(profile: &Profile, outcome: Outcome) -> Report {
    report(
        profile,
        match outcome {
            Outcome::Removed { .. } => Status::HistoricalRemoved,
            Outcome::NoEffect { .. } => Status::HistoricalNoEffect,
        },
    )
}

/// One explicit short-lived worker process only; do not embed this in a daemon.
pub fn apply(profile: &Profile) -> io::Result<Report> {
    let mut controls = controls(profile)?;
    match controls
        .index
        .inspect_effect(&profile.request, &controls.nodes)
        .map_err(|_| invalid())?
    {
        Some(Admission::Historical(outcome)) => return Ok(historical(profile, outcome)),
        Some(Admission::Uncertain(_)) => return Ok(report(profile, Status::Uncertain)),
        _ => (),
    }
    active(profile, &mut controls)?;
    let held = match controls
        .index
        .begin_effect(&profile.request, &mut controls.nodes)
        .map_err(|_| invalid())?
    {
        Admission::Historical(outcome) => return Ok(historical(profile, outcome)),
        Admission::Uncertain(_) => return Ok(report(profile, Status::Uncertain)),
        Admission::New(held) => held,
    };
    let key = Key::from_object(&profile.request.object);
    // No destructive child has been created in preflight. Failures can honestly
    // settle as NoEffect, preserving originals and requiring a new exact decision.
    let preflight = (|| {
        active(profile, &mut controls)?;
        let runner = Runner::new(profile.target.clone())?;
        let binding = bind(&runner, &profile.request.store)?;
        let (keepers, final_loss) = keepers(profile, &mut controls, &key)?;
        let present = hold_copy(&runner, &key, profile.verification_bytes)?.is_some();
        if present && !final_loss {
            // Publish only facts established by the held UUID bindings and
            // verified survivor bytes above. Location logs are not custody;
            // all original protection/domain/policy gates still span drop.
            for survivor in &keepers._bindings {
                if !runner
                    .run(Command::RegisterPresent {
                        key: &key,
                        uuid: &survivor.uuid,
                    })?
                    .success
                {
                    return Err(invalid());
                }
            }
        }
        Ok::<_, io::Error>((runner, binding, keepers, final_loss, present))
    })();
    let (runner, _binding, _keepers, final_loss, present) = match preflight {
        Ok(values) => values,
        Err(_) => {
            held.finish(
                &mut controls.index,
                &mut controls.nodes,
                Outcome::NoEffect {
                    evidence: observation(profile, "refused-before-launch")?,
                },
            )
            .map_err(|_| invalid())?;
            return Ok(report(profile, Status::NoEffect));
        }
    };
    if !present {
        held.finish(
            &mut controls.index,
            &mut controls.nodes,
            Outcome::NoEffect {
                evidence: observation(profile, "already-absent-before-launch")?,
            },
        )
        .map_err(|_| invalid())?;
        return Ok(report(profile, Status::NoEffect));
    }
    // Bulk verification is complete before taking the effective policy/budget
    // holds. They span launch and the provider effect, never mere parsed hashes.
    let _policy = super::policy::hold(
        profile,
        &mut controls.index,
        &controls.nodes,
        controls.intent.review(),
        controls.intent.decision(),
    )?;
    let mut worker = Worker::enter_current_process(controls.index.identity(), &profile.request)?;
    active(profile, &mut controls)?;
    let output = runner.remove(
        &key,
        final_loss,
        &mut worker,
        controls.index.identity(),
        &profile.request,
    );
    if worker.children_stopped(5000).is_err() {
        return Ok(report(profile, Status::Uncertain));
    }
    let output = match output {
        Ok(output) => output,
        Err(_) => return Ok(report(profile, Status::Uncertain)),
    };
    let absent = hold_copy(&runner, &key, profile.verification_bytes)?.is_none();
    if worker.children_stopped(5000).is_err() {
        return Ok(report(profile, Status::Uncertain));
    }
    if successful_key(&output, &key, "drop") && absent {
        held.finish(
            &mut controls.index,
            &mut controls.nodes,
            Outcome::Removed {
                evidence: observation(profile, "per-key-success-and-observed-absence")?,
            },
        )
        .map_err(|_| invalid())?;
        Ok(report(profile, Status::Removed))
    } else if !absent && !output.success {
        held.finish(
            &mut controls.index,
            &mut controls.nodes,
            Outcome::NoEffect {
                evidence: observation(profile, "annex-refused-and-bytes-present")?,
            },
        )
        .map_err(|_| invalid())?;
        Ok(report(profile, Status::NoEffect))
    } else {
        Ok(report(profile, Status::Uncertain))
    }
}

/// Terminate the original named job and reconcile observations. Never retries
/// drop or turns the old decision into authority over reingested bytes.
pub fn reconcile(profile: &Profile, cancel: bool) -> io::Result<Report> {
    let mut controls = controls(profile)?;
    if cancel {
        let owner = retirement_intent::Enrollment {
            owner: profile.intent.owner.clone(),
            subject: profile.intent.subject.clone(),
        };
        let observed = retirement_intent::observe(
            &mut controls.journal,
            &controls.nodes,
            &owner,
            &profile.intent.operation,
        );
        let revision = match observed {
            Ok(current) => Some(current.revision()),
            Err(retirement_intent::Error::Cancelled) => None,
            Err(_) => return Err(invalid()),
        };
        if let Some(revision) = revision {
            let command = ControlKey::parse(&format!(
                "annex-cancel-{}",
                profile.intent.operation.as_str()
            ))
            .map_err(|_| invalid())?;
            retirement_intent::cancel(
                &mut controls.journal,
                &profile.intent.owner,
                &profile.intent.operation,
                revision,
                command,
            )
            .map_err(|_| invalid())?;
        }
        // Cancellation covers the original whole consent, not just this one
        // profile. Job names need only the exact saved scope, never provider URLs.
        let decision = controls.intent.decision();
        let digest = decision.digest().map_err(|_| invalid())?;
        let stop_started = std::time::Instant::now();
        for target in &decision.consent().remove_copies {
            let request = Request {
                operation: operation(&profile.intent.operation, &target.object, &target.store)?,
                object: target.object.clone(),
                store: target.store.clone(),
                context: action_context(&digest, &decision.approval().evidence, &target.store)?,
                expected: profile.request.expected.clone(),
            };
            let remaining = 5000u64
                .saturating_sub(stop_started.elapsed().as_millis().min(u64::MAX as u128) as u64);
            if remaining == 0 {
                return Ok(report(profile, Status::Uncertain));
            }
            if worker::stop(controls.index.identity(), &request, remaining).is_err() {
                return Ok(report(profile, Status::Uncertain));
            }
        }
    }
    let stopped = match worker::stop(controls.index.identity(), &profile.request, 5000) {
        Ok(stopped) => stopped,
        Err(_) => return Ok(report(profile, Status::Uncertain)),
    };
    let had_work = controls
        .index
        .inspect_effect(&profile.request, &controls.nodes)
        .map_err(|_| invalid())?
        .is_some();
    let mut absent = None;
    let outcome =
        controls
            .index
            .reconcile_effect(&profile.request, &stopped, &mut controls.nodes, || {
                let runner = Runner::new(profile.target.clone())
                    .map_err(|_| tig_store::source_claims::protection::Error::Unknown)?;
                let _binding = bind(&runner, &profile.request.store)
                    .map_err(|_| tig_store::source_claims::protection::Error::Unknown)?;
                let missing = hold_copy(
                    &runner,
                    &Key::from_object(&profile.request.object),
                    profile.verification_bytes,
                )
                .map_err(|_| tig_store::source_claims::protection::Error::Unknown)?
                .is_none();
                absent = Some(missing);
                let evidence = observation(
                    profile,
                    if missing {
                        "reconciled-observed-absent-no-retry"
                    } else {
                        "reconciled-observed-present-no-retry"
                    },
                )
                .map_err(|_| tig_store::source_claims::protection::Error::Invalid)?;
                Ok(if missing {
                    Outcome::Removed { evidence }
                } else {
                    Outcome::NoEffect { evidence }
                })
            });
    // Scope/intent validation already succeeded. Failure to settle stopped work
    // is uncertainty, never a completed result or permission to retry removal.
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(_) => return Ok(report(profile, Status::Uncertain)),
    };
    if !had_work {
        Ok(report(profile, Status::NoEffect))
    } else if absent.is_none() {
        Ok(historical(profile, outcome))
    } else {
        Ok(report(
            profile,
            if absent == Some(true) {
                Status::Removed
            } else {
                Status::NoEffect
            },
        ))
    }
}
