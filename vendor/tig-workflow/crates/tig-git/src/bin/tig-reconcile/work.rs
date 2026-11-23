use super::admission;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{
    Digest, Object,
    identity::{canonical_json, parse_json},
};
use tig_store::Key;
pub const SCHEMA: &str = "tig.positive-placement-work.experimental-1";
pub const PROFILED_SCHEMA: &str = "tig.positive-placement-work.experimental-2";
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub schema: String,
    pub operation: Key,
    pub owner: Key,
    pub inspection_digest: Digest,
    pub maximum_actions: u32,
    pub maximum_total_object_bytes: u64,
    pub jobs: Vec<Job>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<BTreeMap<Digest, tig_store::provider::profile::Profile>>,
    pub digest: Digest,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub places: Vec<String>,
    pub admission: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Digest>,
}
pub fn fingerprint(work: &Work) -> Result<Digest, String> {
    let value = if work.schema == PROFILED_SCHEMA {
        json!([
            PROFILED_SCHEMA,
            &work.operation,
            &work.owner,
            &work.inspection_digest,
            work.maximum_actions,
            work.maximum_total_object_bytes,
            &work.jobs,
            &work.profiles
        ])
    } else {
        json!([
            SCHEMA,
            &work.operation,
            &work.owner,
            &work.inspection_digest,
            work.maximum_actions,
            work.maximum_total_object_bytes,
            &work.jobs
        ])
    };
    let bytes = canonical_json(&value).map_err(|_| "work identity budget exceeded")?;
    Ok(Digest::blob(&bytes))
}
pub fn keys(
    operation: &Key,
    owner: &Key,
    inspection: &Digest,
    place: &Digest,
    object: &Object,
) -> Result<(Key, Key), String> {
    let bytes = canonical_json(&json!([
        "tig.positive-placement-job.experimental-1",
        operation,
        owner,
        inspection,
        place,
        object
    ]))
    .map_err(|_| "job identity budget exceeded")?;
    let digest = Digest::blob(&bytes);
    Ok((
        Key::parse(&format!("copy-{}", digest.as_str()))
            .map_err(|_| "invalid publication identity")?,
        Key::parse(&format!("admit-{}", digest.as_str()))
            .map_err(|_| "invalid admission identity")?,
    ))
}
pub fn input(job: &Job) -> Result<admission::profile::Input, String> {
    admission::profile::parse(&serde_json::to_vec(&job.admission).map_err(|_| "invalid work job")?)
}
pub fn validate(work: &Work) -> Result<(), String> {
    if !matches!(work.schema.as_str(), SCHEMA | PROFILED_SCHEMA)
        || (work.schema == PROFILED_SCHEMA) != work.profiles.is_some()
        || work
            .profiles
            .as_ref()
            .is_some_and(|profiles| profiles.len() > 16)
        || !(1..=32).contains(&work.maximum_actions)
        || work.jobs.len() > work.maximum_actions as usize
        || work.maximum_total_object_bytes == 0
        || fingerprint(work)? != work.digest
    {
        return Err("invalid work schema, digest or bounds".into());
    }
    if let Some(profiles) = &work.profiles {
        for (id, profile) in profiles {
            if profile
                .fingerprint()
                .map_err(|_| "invalid work provider profile")?
                != *id
            {
                return Err("work provider fingerprint conflict".into());
            }
        }
    }
    let mut used_profiles = BTreeSet::new();
    let mut tuples = BTreeSet::new();
    let mut ledgers = BTreeMap::new();
    let mut destinations = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    let mut total = 0u64;
    for job in &work.jobs {
        if job.places.is_empty()
            || job.places.len() > 16
            || job.places.iter().any(|p| !token(p))
            || job.places.windows(2).any(|p| p[0] >= p[1])
        {
            return Err("invalid work place aliases".into());
        }
        let input = input(job)?;
        if (work.schema == PROFILED_SCHEMA) != job.provider.is_some() || input.provider.is_some() {
            return Err("work schema/provider reference conflict".into());
        }
        if let Some(id) = &job.provider {
            used_profiles.insert(id.clone());
        }
        if let Some(provider) = provider(work, job)? {
            provider.validate(&input.destination.place, &input.request, &input.job)?;
        }
        for alias in &job.places {
            if let Some(old) = aliases.insert(alias.clone(), input.destination.place.clone())
                && old != input.destination.place
            {
                return Err("place alias binds different destination instances".into());
            }
        }
        let (publication, reservation) = match &job.provider {
            Some(id) => crate::provider_profile::keys(
                &work.operation,
                &work.owner,
                &work.inspection_digest,
                &input.destination.place,
                &input.request.object,
                id,
            )?,
            None => keys(
                &work.operation,
                &work.owner,
                &work.inspection_digest,
                &input.destination.place,
                &input.request.object,
            )?,
        };
        if input.request.owner != work.owner
            || input.request.operation != publication
            || input.job != reservation
            || input.request.required != tig_store::AckLevel::ProcessRestart
            || !tuples.insert((
                input.destination.place.clone(),
                input.request.object.clone(),
            ))
        {
            return Err("work job identity conflict".into());
        }
        let binding = (
            input.destination.root.clone(),
            input.capacity.root.clone(),
            input.capacity.ledger.clone(),
        );
        if let Some(old) = destinations.insert(input.destination.place.clone(), binding.clone())
            && old != binding
        {
            return Err("destination aliases bind different storage".into());
        }
        if let Some(old) = ledgers.insert(input.capacity.ledger, input.capacity.root.clone())
            && old != input.capacity.root
        {
            return Err("capacity identity aliases bind different roots".into());
        }
        total = total
            .checked_add(input.request.object.bytes)
            .filter(|n| *n <= work.maximum_total_object_bytes)
            .ok_or("aggregate object byte ceiling exceeded")?;
    }
    if let Some(profiles) = &work.profiles
        && profiles.keys().cloned().collect::<BTreeSet<_>>() != used_profiles
    {
        return Err("unreferenced or missing work provider profiles".into());
    }
    Ok(())
}
pub fn provider(
    work: &Work,
    job: &Job,
) -> Result<Option<crate::provider_profile::Envelope>, String> {
    match &job.provider {
        None => Ok(None),
        Some(id) => {
            let profile = work
                .profiles
                .as_ref()
                .and_then(|profiles| profiles.get(id))
                .ok_or("missing work provider profile")?;
            Ok(Some(crate::provider_profile::Envelope {
                profile: profile.clone(),
                expected: id.clone(),
                binding: profile.binding.clone(),
                operation: work.operation.clone(),
                inspection_digest: work.inspection_digest.clone(),
            }))
        }
    }
}
pub fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
}
pub fn parse(bytes: &[u8]) -> Result<Work, String> {
    let value = parse_json(bytes).map_err(|_| "invalid or over-budget work JSON")?;
    if value["schema"] == SCHEMA
        && (value.get("profiles").is_some()
            || value["jobs"]
                .as_array()
                .is_some_and(|jobs| jobs.iter().any(|job| job.get("provider").is_some())))
    {
        return Err("provider references require the opt-in work schema".into());
    }
    let work: Work = serde_json::from_value(value).map_err(|_| "invalid work fields")?;
    validate(&work)?;
    Ok(work)
}
