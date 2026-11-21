use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use tig_core::{
    Digest, Object,
    identity::{canonical_json, parse_json},
};
use tig_store::{
    Key, ledger, metadata,
    provider::{custody, profile as provider_profile},
    retention::RetentionGrant,
    source_claims,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub root: PathBuf,
    pub identity: Digest,
    pub minimum_checkpoint: source_claims::Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub root: PathBuf,
    pub minimum_checkpoint: metadata::Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capacity {
    pub root: PathBuf,
    pub ledger: Digest,
    pub minimum_checkpoint: ledger::Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub root: PathBuf,
    pub place: Digest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Survivor {
    pub destination: Place,
    pub capacity: Capacity,
    pub grant: RetentionGrant,
    pub custody_profile: Option<Digest>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub owner: Key,
    pub subject: Digest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub enrollment: Enrollment,
    pub operation: Digest,
    pub decision: Digest,
    pub object: Object,
    pub attempt: u64,
    pub destination: Place,
    pub capacity: Capacity,
    pub index: Authority,
    pub journal: Authority,
    pub metadata: Metadata,
    pub preservation: Option<custody::Profile>,
    pub survivors: Vec<Survivor>,
    pub maximum_object_bytes: u64,
    pub allow_cooperative: bool,
    pub custody_profiles: Option<BTreeMap<Digest, provider_profile::Profile>>,
    pub intent_execution_profile: Option<Digest>,
}
pub struct Parsed {
    pub profile: Profile,
    pub digest: Digest,
}
pub fn parse(bytes: &[u8]) -> Result<Parsed, String> {
    if bytes.len() > 61_440 {
        return Err("removal profile byte budget".into());
    }
    let value = parse_json(bytes).map_err(|_| "invalid/over-budget removal JSON")?;
    let version = match value["schema"].as_str() {
        Some("tig.removal-execution-profile.experimental-1") => {
            if value.get("intent_execution_profile").is_some()
                || value.get("custody_profiles").is_some()
                || value["survivors"]
                    .as_array()
                    .is_some_and(|ss| ss.iter().any(|s| s.get("custody_profile").is_some()))
            {
                return Err("legacy removal profile cannot name custody profiles".into());
            }
            1
        }
        Some("tig.removal-execution-profile.experimental-2") => {
            if value.get("intent_execution_profile").is_some() {
                return Err("v2 cannot name an intent execution profile".into());
            }
            2
        }
        Some("tig.removal-execution-profile.experimental-3") => {
            if value.get("custody_profiles").is_some()
                || value["survivors"]
                    .as_array()
                    .is_some_and(|ss| ss.iter().any(|s| s.get("custody_profile").is_some()))
            {
                return Err("v3 restores profiles from admitted intent only".into());
            }
            3
        }
        _ => return Err("unsupported removal profile schema".into()),
    };
    let digest = Digest::blob(
        &canonical_json(&serde_json::json!([
            match version {
                1 => "tig.removal-execution-input.experimental-1",
                2 => "tig.removal-execution-input.experimental-2",
                _ => "tig.removal-execution-input.experimental-3",
            },
            value
        ]))
        .map_err(|_| "removal fingerprint budget")?,
    );
    let p: Profile = serde_json::from_value(value).map_err(|_| "invalid removal profile fields")?;
    if !p.allow_cooperative
        || p.attempt == 0
        || p.maximum_object_bytes == 0
        || p.object.bytes > p.maximum_object_bytes
        || p.survivors.len() > 16
        || p.index.identity == p.journal.identity
        || [
            &p.destination.root,
            &p.capacity.root,
            &p.index.root,
            &p.journal.root,
            &p.metadata.root,
        ]
        .iter()
        .any(|r| !r.is_absolute())
    {
        return Err("unsupported removal profile, bounds or bindings".into());
    }
    if version == 3 && p.intent_execution_profile.is_none() {
        return Err("missing saved intent execution profile".into());
    }
    if version == 2 {
        let records = p
            .custody_profiles
            .as_ref()
            .ok_or("missing custody profiles")?;
        if records.is_empty()
            || records.len() > 16
            || p.preservation.is_none()
            || p.survivors.is_empty()
        {
            return Err("custody profiles require retained removal and bounded records".into());
        }
        let mut used = BTreeSet::new();
        for s in &p.survivors {
            let id = s
                .custody_profile
                .as_ref()
                .ok_or("missing survivor custody profile")?;
            let record = records.get(id).ok_or("unresolved custody profile")?;
            if record.binding.place != s.destination.place
                || record
                    .fingerprint()
                    .map_err(|_| "invalid custody profile")?
                    != *id
            {
                return Err("custody profile fingerprint/place mismatch".into());
            }
            used.insert(id);
        }
        if used.len() != records.len() {
            return Err("unused custody profiles".into());
        }
    }
    let mut found = BTreeSet::new();
    for s in &p.survivors {
        if !s.destination.root.is_absolute()
            || !s.capacity.root.is_absolute()
            || s.destination.place == p.destination.place
            || !found.insert(s.destination.place.clone())
            || s.grant.store() != &s.destination.place
            || s.grant.request().publication.object != p.object
        {
            return Err("foreign/duplicate survivor binding".into());
        }
        // Deserialize alone does not validate the immutable grant fingerprint.
        RetentionGrant::from_json(&s.grant.to_json().map_err(|_| "invalid grant")?)
            .map_err(|_| "invalid retained grant")?;
    }
    match &p.preservation {
        Some(profile) => {
            if !profile.allow_cooperative
                || profile
                    .places(&p.destination.place, &p.object)
                    .map_err(|_| "invalid preservation profile")?
                    != found
                || profile.retained.iter().any(|r| {
                    !p.survivors
                        .iter()
                        .any(|s| custody::native::commitment(&s.grant) == r.commitment)
                })
            {
                return Err("preservation/endpoint scope mismatch".into());
            }
        }
        None if !p.survivors.is_empty() => {
            return Err("final-loss profile cannot name survivors".into());
        }
        None => {}
    }
    Ok(Parsed { profile: p, digest })
}
