use super::{
    inspection, source_config,
    work::{self, Job, Work},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{
    Digest, Object,
    identity::{canonical_json, parse_json},
};
use tig_store::{
    Key,
    local_enrollment::{CapacityBinding, Destination},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    operation: Key,
    owner: Key,
    inspection: Value,
    bindings: BTreeMap<String, Binding>,
    #[serde(default)]
    profiles: Option<BTreeMap<Digest, tig_store::provider::profile::Profile>>,
    sources: Vec<Source>,
    maximum_actions: u32,
    maximum_total_object_bytes: u64,
    maximum_peak_per_action: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    destination: Destination,
    capacity: CapacityBinding,
    #[serde(default)]
    provider: Option<Digest>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    object: Object,
    config: source_config::Config,
}
#[derive(Deserialize)]
struct Requirements {
    input_digest: Digest,
    source_metadata_complete: bool,
    required_without_supplied_copy: Vec<Placement>,
    desired_with_supplied_copy: Vec<Placement>,
}
#[derive(Deserialize)]
struct Placement {
    object: Object,
    place: String,
}
pub fn compile(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let value = parse_json(bytes).map_err(|_| "invalid or over-budget placement profile")?;
    if value["schema"] == "tig.positive-placement-profile.experimental-1"
        && (value.get("profiles").is_some()
            || value["bindings"].as_object().is_some_and(|bindings| {
                bindings
                    .values()
                    .any(|binding| binding.get("provider").is_some())
            }))
    {
        return Err("provider references require the opt-in placement schema".into());
    }
    let profile: Profile =
        serde_json::from_value(value).map_err(|_| "invalid placement profile fields")?;
    if !matches!(
        profile.schema.as_str(),
        "tig.positive-placement-profile.experimental-1"
            | "tig.positive-placement-profile.experimental-2"
    ) || profile.schema.ends_with("-2") != profile.profiles.is_some()
        || profile
            .profiles
            .as_ref()
            .is_some_and(|profiles| profiles.len() > 16)
        || profile
            .bindings
            .values()
            .any(|b| b.provider.is_some() != profile.profiles.is_some())
        || profile.bindings.len() > 16
        || profile.sources.len() > 32
        || !(1..=32).contains(&profile.maximum_actions)
        || profile.maximum_total_object_bytes == 0
        || profile.maximum_peak_per_action == 0
        || profile.bindings.keys().any(|p| !work::token(p))
    {
        return Err("unsupported placement profile or budgets".into());
    }
    if let Some(profiles) = &profile.profiles {
        for (id, p) in profiles {
            if p.fingerprint()
                .map_err(|_| "invalid placement provider profile")?
                != *id
            {
                return Err("placement provider fingerprint conflict".into());
            }
        }
    }
    let mut sources = BTreeMap::new();
    for source in &profile.sources {
        source.config.validate(&source.object)?;
        if sources
            .insert(source.object.clone(), &source.config)
            .is_some()
        {
            return Err("duplicate object source binding".into());
        }
    }
    let inspection = inspection::profile::parse(
        &serde_json::to_vec(&profile.inspection).map_err(|_| "invalid inspection input")?,
    )?;
    let (report, complete) = inspection::report::inspect(inspection)?;
    if !complete {
        return Err("incomplete source metadata; no executable work compiled".into());
    }
    let requirements: Requirements =
        serde_json::from_slice(&report).map_err(|_| "cannot decode internal requirements")?;
    if !requirements.source_metadata_complete {
        return Err("incomplete source metadata".into());
    }
    // Supplied copies remain unverified. BOTH desired partitions need native
    // reconciliation; only the existing publisher may decide warm reuse.
    let mut grouped: BTreeMap<(Digest, Object), BTreeSet<String>> = BTreeMap::new();
    for placement in requirements
        .required_without_supplied_copy
        .into_iter()
        .chain(requirements.desired_with_supplied_copy)
    {
        let binding = profile
            .bindings
            .get(&placement.place)
            .ok_or("desired place has no explicit binding")?;
        grouped
            .entry((binding.destination.place.clone(), placement.object))
            .or_default()
            .insert(placement.place);
        if grouped.len() > profile.maximum_actions as usize {
            return Err("aggregate action ceiling exceeded".into());
        }
    }
    let mut jobs = Vec::new();
    let mut selected_profiles = BTreeMap::new();
    for ((place, object), aliases) in grouped {
        let mut bindings = aliases
            .iter()
            .map(|p| profile.bindings.get(p).ok_or("missing binding"));
        let binding = bindings.next().ok_or("empty place aliases")??;
        for other in bindings {
            let other = other?;
            if binding.provider != other.provider
                || binding.destination.root != other.destination.root
                || binding.capacity.root != other.capacity.root
                || binding.capacity.ledger != other.capacity.ledger
                || binding.capacity.minimum_checkpoint != other.capacity.minimum_checkpoint
            {
                return Err("aliased place bindings or witnesses differ".into());
            }
        }
        let source = sources
            .get(&object)
            .ok_or("desired object has no exact source binding")?;
        let (operation, job) = if let Some(id) = &binding.provider {
            let accepted = profile
                .profiles
                .as_ref()
                .and_then(|profiles| profiles.get(id))
                .ok_or("missing desired provider profile")?;
            if accepted.binding.place != place {
                return Err("provider profile place conflict".into());
            }
            selected_profiles.insert(id.clone(), accepted.clone());
            crate::provider_profile::keys(
                &profile.operation,
                &profile.owner,
                &requirements.input_digest,
                &place,
                &object,
                id,
            )?
        } else {
            work::keys(
                &profile.operation,
                &profile.owner,
                &requirements.input_digest,
                &place,
                &object,
            )?
        };
        jobs.push(Job {provider:binding.provider.clone(),places:aliases.into_iter().collect(),admission:json!({"schema":"tig.publication-admission-profile.experimental-1",
            "destination":binding.destination,"capacity":binding.capacity,"job":job,"maximum_peak":profile.maximum_peak_per_action,
            "request":{"operation":operation,"owner":profile.owner,"object":object,"required":"process_restart"},"source":source})});
    }
    let mut work = Work {
        schema: if profile.profiles.is_some() {
            work::PROFILED_SCHEMA.into()
        } else {
            work::SCHEMA.into()
        },
        operation: profile.operation,
        owner: profile.owner,
        inspection_digest: requirements.input_digest,
        maximum_actions: profile.maximum_actions,
        maximum_total_object_bytes: profile.maximum_total_object_bytes,
        jobs,
        profiles: profile.profiles.map(|_| selected_profiles),
        digest: Digest::blob(b""),
    };
    work.digest = work::fingerprint(&work)?;
    work::validate(&work)?;
    canonical_json(&serde_json::to_value(work).map_err(|_| "cannot encode work")?)
        .map_err(|_| "compiled work exceeds metadata budget".into())
}
