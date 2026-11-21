//! Opt-in standing policy over annex. Existing registry, budget and intent engines.
use super::{
    control::Package,
    invalid,
    retirement::{self, Custody, MetadataBinding, Profile},
};
use crate::annex::process::Enrollment;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::{self, Read},
    path::PathBuf,
};
use tig_core::{
    Digest, Object,
    retirement::review::{
        Review,
        decision::Decision,
        routine_policy::{self, Policy, PolicyConfig, Run, RunConfig},
    },
    tree::NodeStore,
};
use tig_store::{
    Key,
    metadata::MetadataStore,
    provider::profile::{self, Qualification},
    retirement_intent::policy::GrantRef,
    source_claims::{
        Checkpoint, Version,
        policy_registry::{self as registry, PlaceScope, ProviderCatalogue, Registry},
        protection::Index,
    },
};
const SCHEMA: &str = "tig.annex-policy-work.design-1";
fn encoded(value: &impl Serialize) -> io::Result<Vec<u8>> {
    tig_core::identity::canonical_json(&serde_json::to_value(value).map_err(|_| invalid())?)
        .map_err(|_| invalid())
}
fn load(nodes: &impl NodeStore, object: &Object) -> io::Result<Vec<u8>> {
    if object.bytes == 0 || object.bytes > 65536 {
        return Err(invalid());
    }
    let bytes = nodes
        .get(object)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if Object::from_bytes(&bytes) != *object {
        return Err(invalid());
    }
    Ok(bytes)
}
pub(super) fn review(path: &PathBuf) -> io::Result<Review> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(invalid());
    }
    Review::from_json(&bytes).map_err(|_| invalid())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub root: PathBuf,
    pub minimum: Checkpoint,
    pub registry: Digest,
    pub slot: Key,
    pub version: Version,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub authority: Authority,
    pub proof: Object,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    grant: GrantRef,
    policy: Object,
    catalogue: Object,
    run: Object,
}
struct Work {
    record: Record,
    bytes: Vec<u8>,
    policy: Policy,
    catalogue: ProviderCatalogue,
    run: Run,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enable {
    pub package: Package,
    pub root: PathBuf,
    pub epoch: Key,
    pub slot: Key,
    pub actor: Key,
    pub placement_policy: Digest,
    pub version: u64,
    pub removable: BTreeSet<Digest>,
    pub required_survivors: BTreeSet<Digest>,
    pub limits: routine_policy::Limits,
    pub custody: Custody,
    pub providers: Vec<Enrollment>,
}
#[derive(Serialize)]
pub struct Enabled {
    pub authority: Authority,
    pub metadata: MetadataBinding,
}
/// Stable trusted transport configuration; advancing checkpoint witnesses are
/// deliberately excluded. No filesystem path or executable comes from asset data.
fn transport(e: &Enrollment) -> io::Result<Digest> {
    let c = e.control.as_ref().ok_or_else(invalid)?;
    Ok(Digest::blob(&encoded(&(
        SCHEMA,
        &e.executable,
        &e.git,
        &e.repository,
        &e.tools,
        &e.empty_config,
        &e.empty_hooks,
        &e.temporary,
        &e.legacy_profile,
        e.command_millis,
        e.output_bytes,
        &c.root,
        &c.index,
        &c.store,
    ))?))
}
fn catalogue(input: &Enable, index: &Index) -> io::Result<ProviderCatalogue> {
    let mut profiles = std::collections::BTreeMap::new();
    let mut places = std::collections::BTreeMap::new();
    if input.custody.domains.keys().cloned().collect::<Vec<_>>()
        != index.enrollment_profile().stores
    {
        return Err(invalid());
    }
    for e in &input.providers {
        let c = e.control.as_ref().ok_or_else(invalid)?;
        if c.index != *index.identity()
            || c.root != input.package.index_root
            || places.contains_key(&c.store)
        {
            return Err(invalid());
        }
        let domain = input.custody.domains.get(&c.store).ok_or_else(invalid)?;
        places.insert(
            c.store.clone(),
            PlaceScope {
                capacity_domain: c.store.clone(),
                failure_domain: Key::parse(domain.as_str()).map_err(|_| invalid())?,
            },
        );
        // These are configuration identities only. Native provider capabilities
        // remain Unsupported; annex execution uses its separate admitted gates.
        let p = profile::Profile {
            schema: profile::Schema::Experimental1,
            binding: profile::Binding {
                place: c.store.clone(),
                authority: transport(e)?,
                generation: 1,
            },
            adapter: Key::parse("tig-annex-local").map_err(|_| invalid())?,
            adapter_build: Digest::blob(SCHEMA.as_bytes()),
            provider: Key::parse("git-annex-local").map_err(|_| invalid())?,
            provider_build: Digest::blob(b"explicit-local-annex-enrollment"),
            read: Qualification::Unsupported,
            publish: Qualification::Unsupported,
            custody: Qualification::Unsupported,
            issue: Qualification::Unsupported,
            release: Qualification::Unsupported,
            remove: Qualification::Unsupported,
        };
        profiles.insert(p.fingerprint().map_err(|_| invalid())?, p);
    }
    if places.keys().cloned().collect::<Vec<_>>() != index.enrollment_profile().stores {
        return Err(invalid());
    }
    ProviderCatalogue::new(profiles, places).map_err(|_| invalid())
}
pub fn enable(input: &Enable) -> io::Result<Enabled> {
    if !input.root.is_absolute() || input.root.exists() || input.version == 0 {
        return Err(invalid());
    }
    let index = Index::open(&input.package.index_root, &input.package.index_minimum)
        .map_err(|_| invalid())?;
    if index.identity() != &input.package.index {
        return Err(invalid());
    }
    let catalogue = catalogue(input, &index)?;
    if input.custody.minimum_domains as usize != input.required_survivors.len() {
        return Err(invalid());
    }
    let domains: BTreeSet<_> = input
        .required_survivors
        .iter()
        .map(|s| input.custody.domains.get(s).cloned().ok_or_else(invalid))
        .collect::<io::Result<_>>()?;
    if domains.len() != input.required_survivors.len()
        || input.removable.iter().any(|s| {
            input
                .custody
                .domains
                .get(s)
                .is_none_or(|d| domains.contains(d))
        })
    {
        return Err(invalid());
    }
    let catalogue_bytes = catalogue.to_json().map_err(|_| invalid())?;
    let policy = Policy::new(PolicyConfig {
        namespace: index.identity().clone(),
        owner: index.enrollment_profile().subject.clone(),
        source: index.policy_source(&input.actor).map_err(|_| invalid())?,
        authority: index.identity().clone(),
        epoch: index.identity().clone(),
        version: input.version,
        placement_policy: input.placement_policy.clone(),
        provider_bindings: Digest::blob(&catalogue_bytes),
        removable: input.removable.clone(),
        required_survivors: input.required_survivors.clone(),
        limits: input.limits.clone(),
    })
    .map_err(|_| invalid())?;
    let binding =
        registry::Binding::from_effect_index(&index, input.actor.clone(), vec![input.slot.clone()])
            .map_err(|_| invalid())?;
    let mut nodes = MetadataStore::open(
        &input.package.metadata.root,
        &input.package.metadata.minimum,
    )
    .map_err(|_| invalid())?;
    let policy_root = nodes
        .put(&policy.to_json().map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let catalogue_root = nodes.put(&catalogue_bytes).map_err(|_| invalid())?;
    nodes.finish().map_err(|_| invalid())?;
    let mut registry = Registry::create_accounted(&input.root, input.epoch.clone(), binding)
        .map_err(|_| invalid())?;
    registry
        .update(
            &registry::Update {
                slot: input.slot.clone(),
                owner: index.enrollment_profile().owner.clone(),
                operation: Key::parse("enable").map_err(|_| invalid())?,
                expected_revision: 0,
                registration: registry::Registration::new(policy_root, catalogue_root)
                    .map_err(|_| invalid())?,
            },
            &mut nodes,
        )
        .map_err(|_| invalid())?;
    let version = registry
        .state(&input.slot)
        .map_err(|_| invalid())?
        .and_then(|s| s.current)
        .ok_or_else(invalid)?;
    Ok(Enabled {
        authority: Authority {
            root: input.root.clone(),
            minimum: registry.checkpoint(),
            registry: registry.identity().clone(),
            slot: input.slot.clone(),
            version,
        },
        metadata: MetadataBinding {
            root: input.package.metadata.root.clone(),
            minimum: nodes.checkpoint(),
        },
    })
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub base: Profile,
    pub authority: Authority,
    pub review_input: PathBuf,
    pub catalogue: Digest,
    pub operation: Digest,
}
fn open(a: &Authority, index: &Index) -> io::Result<Registry> {
    if !a.root.is_absolute() {
        return Err(invalid());
    }
    let r = Registry::open(&a.root, &a.minimum).map_err(|_| invalid())?;
    if r.identity() != &a.registry {
        return Err(invalid());
    }
    r.require_effect_index(index).map_err(|_| invalid())?;
    Ok(r)
}
pub fn stage(input: &Stage) -> io::Result<Vec<Profile>> {
    stage_with_hook(input, |_| Ok(()))
}
/// The same staging path with caller observations at durable intent boundaries.
/// No CLI environment switch or payload-execution bypass is introduced.
pub fn stage_with_hook(
    input: &Stage,
    hook: impl FnMut(retirement::StagingPoint) -> io::Result<()>,
) -> io::Result<Vec<Profile>> {
    if input.base.policy.is_some() {
        return Err(invalid());
    }
    let c = input.base.target.control.as_ref().ok_or_else(invalid)?;
    let mut index = Index::open(&c.root, &c.minimum).map_err(|_| invalid())?;
    let mut nodes = MetadataStore::open(&input.base.metadata.root, &input.base.metadata.minimum)
        .map_err(|_| invalid())?;
    let mut r = open(&input.authority, &index)?;
    let held = r
        .hold(
            &input.authority.slot,
            Some(&input.authority.version),
            &nodes,
        )
        .map_err(|_| invalid())?;
    let reviewed = review(&input.review_input)?;
    let grant = GrantRef {
        registry: r.identity().clone(),
        slot: held.slot().clone(),
        version: held.version().clone(),
    };
    let run = Run::new(RunConfig {
        namespace: index.identity().clone(),
        frame: r.effect_frame(&mut index, &nodes).map_err(|_| invalid())?,
        catalogue: input.catalogue.clone(),
        provider_bindings: held.policy().config().provider_bindings.clone(),
        admission: grant.fingerprint().map_err(|_| invalid())?,
        native_sources: BTreeSet::new(),
    })
    .map_err(|_| invalid())?;
    let proposal =
        routine_policy::compile(held.policy(), &run, &reviewed).map_err(|_| invalid())?;
    if !proposal.has_candidates() {
        return Err(invalid());
    }
    let record = Record {
        schema: SCHEMA.into(),
        grant,
        policy: nodes
            .put(&held.policy().to_json().map_err(|_| invalid())?)
            .map_err(|_| invalid())?,
        catalogue: nodes
            .put(&held.catalogue().to_json().map_err(|_| invalid())?)
            .map_err(|_| invalid())?,
        run: nodes
            .put(&run.to_json().map_err(|_| invalid())?)
            .map_err(|_| invalid())?,
    };
    let proof = nodes.put(&encoded(&record)?).map_err(|_| invalid())?;
    nodes.finish().map_err(|_| invalid())?;
    drop(held);
    let mut base = input.base.clone();
    base.policy = Some(Binding {
        authority: input.authority.clone(),
        proof,
    });
    base.metadata.minimum = nodes.checkpoint();
    retirement::stage_bound_hook(
        &retirement::StageProfile {
            base,
            review_input: input.review_input.clone(),
            consent: proposal.proposed_consent().clone(),
            operation: input.operation.clone(),
        },
        true,
        hook,
    )
}
fn work(
    profile: &Profile,
    nodes: &impl NodeStore,
    review: &Review,
    decision: Option<&Decision>,
) -> io::Result<Work> {
    let b = profile.policy.as_ref().ok_or_else(invalid)?;
    let bytes = load(nodes, &b.proof)?;
    if bytes.len() > 4096 {
        return Err(invalid());
    }
    let record: Record =
        serde_json::from_value(tig_core::identity::parse_json(&bytes).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
    if record.schema != SCHEMA
        || encoded(&record)? != bytes
        || record.grant.registry != b.authority.registry
        || record.grant.slot != b.authority.slot
        || record.grant.version != b.authority.version
    {
        return Err(invalid());
    }
    let policy = Policy::from_json(&load(nodes, &record.policy)?).map_err(|_| invalid())?;
    let catalogue =
        ProviderCatalogue::from_json(&load(nodes, &record.catalogue)?).map_err(|_| invalid())?;
    let run = Run::from_json(&load(nodes, &record.run)?).map_err(|_| invalid())?;
    let registration = registry::Registration::new(record.policy.clone(), record.catalogue.clone())
        .map_err(|_| invalid())?;
    if Object::from_bytes(&encoded(&registration)?) != record.grant.version.selection.root
        || policy.config().provider_bindings != record.catalogue.sha256
        || run.config().admission != record.grant.fingerprint().map_err(|_| invalid())?
        || !run.config().native_sources.is_empty()
    {
        return Err(invalid());
    }
    if profile.custody.minimum_domains as usize != policy.config().required_survivors.len() {
        return Err(invalid());
    }
    let required: BTreeSet<_> = policy
        .config()
        .required_survivors
        .iter()
        .map(|s| profile.custody.domains.get(s).cloned().ok_or_else(invalid))
        .collect::<io::Result<_>>()?;
    if required.len() != policy.config().required_survivors.len()
        || policy.config().removable.iter().any(|s| {
            profile
                .custody
                .domains
                .get(s)
                .is_none_or(|d| required.contains(d))
        })
    {
        return Err(invalid());
    }
    let proposal = routine_policy::compile(&policy, &run, review).map_err(|_| invalid())?;
    if decision.is_some_and(|d| proposal.proposed_consent() != d.consent()) {
        return Err(invalid());
    }
    for (place, scope) in catalogue.places() {
        if profile.custody.domains.get(place).map(|d| d.as_str())
            != Some(scope.failure_domain.as_str())
        {
            return Err(invalid());
        }
    }
    for e in std::iter::once(&profile.target).chain(&profile.survivors) {
        let c = e.control.as_ref().ok_or_else(invalid)?;
        let matching: Vec<_> = catalogue
            .profiles()
            .values()
            .filter(|p| p.binding.place == c.store)
            .collect();
        if matching.len() != 1 || matching[0].binding.authority != transport(e)? {
            return Err(invalid());
        }
    }
    Ok(Work {
        record,
        bytes,
        policy,
        catalogue,
        run,
    })
}
pub(super) fn evidence(
    profile: &Profile,
    nodes: &impl NodeStore,
    review: &Review,
    base: Digest,
) -> io::Result<Digest> {
    let Some(binding) = &profile.policy else {
        return Ok(base);
    };
    work(profile, nodes, review, None)?;
    Ok(Digest::blob(&encoded(&(SCHEMA, base, &binding.proof))?))
}
pub(super) fn verify(
    profile: &Profile,
    nodes: &impl NodeStore,
    review: &Review,
    decision: &Decision,
) -> io::Result<()> {
    if profile.policy.is_some() {
        work(profile, nodes, review, Some(decision))?;
    }
    Ok(())
}
pub(super) fn reserve(
    profile: &Profile,
    index: &mut Index,
    nodes: &mut MetadataStore,
    review: &Review,
    decision: &Decision,
) -> io::Result<Option<Binding>> {
    let Some(binding) = &profile.policy else {
        return Ok(None);
    };
    let w = work(profile, nodes, review, Some(decision))?;
    let mut r = open(&binding.authority, index)?;
    let held = r
        .hold(
            &binding.authority.slot,
            Some(&w.record.grant.version),
            nodes,
        )
        .map_err(|_| invalid())?;
    if held.policy() != &w.policy || held.catalogue() != &w.catalogue {
        return Err(invalid());
    }
    r.reserve_effect_run(
        &held,
        index,
        &registry::effects::Admission {
            run: &w.run,
            review,
            decision,
            context: &w.bytes,
        },
        nodes,
    )
    .map_err(|_| invalid())?;
    let mut result = binding.clone();
    result.authority.minimum = r.checkpoint();
    Ok(Some(result))
}
pub(super) struct Lease {
    _policy: registry::HeldPolicy,
    _budget: registry::effects::HeldRun,
}
pub(super) fn hold(
    profile: &Profile,
    index: &mut Index,
    nodes: &impl NodeStore,
    review: &Review,
    decision: &Decision,
) -> io::Result<Option<Lease>> {
    let Some(binding) = &profile.policy else {
        return Ok(None);
    };
    let w = work(profile, nodes, review, Some(decision))?;
    let mut r = open(&binding.authority, index)?;
    let held = r
        .hold(
            &binding.authority.slot,
            Some(&w.record.grant.version),
            nodes,
        )
        .map_err(|_| invalid())?;
    if held.policy() != &w.policy || held.catalogue() != &w.catalogue {
        return Err(invalid());
    }
    let budget = r
        .hold_effect_run(
            &held,
            index,
            &registry::effects::Admission {
                run: &w.run,
                review,
                decision,
                context: &w.bytes,
            },
            nodes,
        )
        .map_err(|_| invalid())?;
    Ok(Some(Lease {
        _policy: held,
        _budget: budget,
    }))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revoke {
    pub authority: Authority,
    pub owner: Key,
    pub operation: Key,
}
pub fn revoke(input: &Revoke) -> io::Result<Authority> {
    if !input.authority.root.is_absolute() {
        return Err(invalid());
    }
    let mut r =
        Registry::open(&input.authority.root, &input.authority.minimum).map_err(|_| invalid())?;
    if r.identity() != &input.authority.registry {
        return Err(invalid());
    }
    r.revoke(
        &input.authority.slot,
        &input.owner,
        input.authority.version.revision,
        input.operation.clone(),
    )
    .map_err(|_| invalid())?;
    let mut result = input.authority.clone();
    result.minimum = r.checkpoint();
    Ok(result)
}
