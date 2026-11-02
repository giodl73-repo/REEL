//! Bounded explicit project restoration over a single frozen native Git view.
use crate::{
    restore,
    source::{GitId, Limits as GitLimits, Repository, Scope, native::Limits as NativeLimits},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Instant,
};
use tig_assets::{
    graph::Reference,
    resolve::{AssetResolver, Resolved},
};
use tig_core::{
    Digest, LogicalPath, Object,
    identity::{canonical_json, parse_json},
};
use tig_store::existing::{ByteSource, Profile as ByteProfile, Protection, ReadCache};

type Result<T> = std::result::Result<T, String>;

pub mod partials;
pub mod progress;
pub mod requirements;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    git_executable: PathBuf,
    repository: PathBuf,
    commit: String,
    destination_root: PathBuf,
    byte_source: ByteProfile,
    limits: Limits,
    #[serde(default)]
    metadata_limits: NativeLimits,
    #[serde(default)]
    host_files: BTreeMap<String, PathBuf>,
    capacity_root: Option<PathBuf>,
    #[serde(default)]
    consumer_evidence: BTreeMap<String, Object>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    entries: usize,
    destination_bytes: u64,
    #[serde(default = "requirements::default_host_budget")]
    host_verification_bytes: u64,
    #[serde(default = "partials::default_directory_entries")]
    partial_directory_entries: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    records: Vec<String>,
    definitions: Vec<String>,
    destination_prefixes: Vec<LogicalPath>,
    selection: Selection,
    #[serde(default)]
    requirements: requirements::Requirements,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Selection {
    Entries {
        entries: Vec<Entry>,
    },
    Catalog {
        selector: Selector,
        destination_field: String,
    },
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    selector: Selector,
    destination: LogicalPath,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Selector {
    Selected,
    Stream { slot: String },
}

#[derive(Serialize)]
struct Item {
    record: Reference,
    definition: Reference,
    selector: Selector,
    object: Object,
    destination: LogicalPath,
}

/// Frozen metadata only. No provider guard is held and no destination is created.
pub struct Plan {
    identity: Digest,
    source_commit: String,
    checkpoint: Digest,
    profile_object: Object,
    manifest_object: Object,
    manifest_path: LogicalPath,
    profile: Profile,
    items: Vec<Item>,
    unique: BTreeMap<Digest, u64>,
    destination_bytes: u64,
    metadata_millis: u64,
    requirements: requirements::Requirements,
}

#[derive(Serialize)]
pub struct EntryResult {
    pub ordinal: usize,
    pub record: Reference,
    pub object: Object,
    pub selector: Selector,
    pub destination: LogicalPath,
    pub action: &'static str,
    pub destination_verified: bool,
    pub source_protection_observed_at_copy: Option<Protection>,
}
#[derive(Serialize)]
pub struct Failure {
    pub ordinal: Option<usize>,
    pub phase: &'static str,
    pub reason: String,
}
#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub source_commit: String,
    pub source_checkpoint: Digest,
    pub profile: Object,
    pub manifest: Object,
    pub manifest_path: LogicalPath,
    pub plan: Digest,
    pub apply: bool,
    pub unique_records: usize,
    pub unique_objects: usize,
    pub unique_object_bytes: u64,
    pub destination_bytes: u64,
    pub copied_bytes: u64,
    pub source_verifications: u32,
    pub logical_source_verification_bytes: u64,
    pub preflight_protection_modes: Vec<Protection>,
    pub metadata_millis: u64,
    pub preflight_millis: u64,
    pub copy_millis: u64,
    pub entries: Vec<EntryResult>,
    pub host_files: Vec<requirements::HostObservation>,
    pub capacity: Option<requirements::CapacityObservation>,
    pub consumer_evidence: BTreeMap<String, Object>,
    pub consumer_evidence_verified: bool,
    pub failure: Option<Failure>,
    pub source_guards_held: bool,
    pub independent_custody_verified: bool,
    pub authorizes_eviction: bool,
    pub complete_semantic_coverage: bool,
}

fn scope(paths: Vec<String>) -> Result<Scope> {
    Scope::new(
        paths
            .iter()
            .map(|p| LogicalPath::parse(p))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| "invalid source scope")?,
    )
    .map_err(|e| e.to_string())
}
fn elapsed(start: Instant) -> u64 {
    start.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}
fn select(asset: &Resolved, selector: &Selector) -> Result<Object> {
    match selector {
        Selector::Selected => asset.selected(),
        Selector::Stream { slot } => asset.stream(slot),
    }
    .cloned()
    .map_err(|_| "requested asset selection unavailable".into())
}

pub fn prepare(raw_profile: &[u8], manifest_path: &str) -> Result<Plan> {
    let start = Instant::now();
    let profile: Profile =
        serde_json::from_value(parse_json(raw_profile).map_err(|_| "invalid profile JSON")?)
            .map_err(|_| "invalid project profile")?;
    if profile.schema != "tig.project-restore-profile.experimental-1"
        || !(1..=1024).contains(&profile.limits.entries)
        || profile.limits.destination_bytes > 8 * 1024 * 1024 * 1024
        || !(1..=16384).contains(&profile.limits.partial_directory_entries)
        || !profile.destination_root.is_absolute()
        || profile.destination_root.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err("project profile bounds or destination root invalid".into());
    }
    if profile.consumer_evidence.len() > 16
        || profile
            .consumer_evidence
            .keys()
            .any(|label| label.len() > 128 || LogicalPath::parse(label).is_err())
    {
        return Err("consumer evidence labels or count invalid".into());
    }
    ReadCache::new(profile.byte_source.clone()).map_err(|e| e.to_string())?;
    let repository = Repository::new(
        profile.git_executable.clone(),
        profile.repository.clone(),
        GitLimits::default(),
    )
    .map_err(|e| e.to_string())?;
    let view = repository
        .commit(GitId::parse(&profile.commit).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let manifest_path = LogicalPath::parse(manifest_path).map_err(|_| "invalid manifest path")?;
    let raw_manifest = repository
        .document(&view, &manifest_path)
        .map_err(|e| e.to_string())?;
    let manifest: Manifest =
        serde_json::from_value(parse_json(&raw_manifest).map_err(|_| "invalid manifest JSON")?)
            .map_err(|_| "invalid project manifest")?;
    if manifest.schema != "tig.project-restore.experimental-2"
        || manifest.destination_prefixes.is_empty()
        || manifest.destination_prefixes.len() > 32
    {
        return Err("invalid manifest scope".into());
    }
    let assets = repository
        .native(
            &view,
            scope(manifest.records)?,
            scope(manifest.definitions)?,
            profile.metadata_limits,
        )
        .map_err(|e| e.to_string())?;
    let entries = match manifest.selection {
        Selection::Entries { entries } => entries,
        Selection::Catalog {
            selector,
            destination_field,
        } => {
            if destination_field.is_empty()
                || destination_field.len() > 128
                || destination_field.chars().any(char::is_control)
            {
                return Err("invalid destination property".into());
            }
            let mut entries = Vec::new();
            for asset in assets.selected_records() {
                if entries.len() >= profile.limits.entries {
                    return Err("requested entry budget exceeded".into());
                }
                let destination = asset
                    .authored()
                    .get(&destination_field)
                    .and_then(serde_json::Value::as_str)
                    .ok_or("declared destination property unavailable")?;
                entries.push(Entry {
                    id: asset.reference().id.clone(),
                    selector: selector.clone(),
                    destination: LogicalPath::parse(destination)
                        .map_err(|_| "invalid declared destination")?,
                });
            }
            entries
        }
    };
    if entries.is_empty() || entries.len() > profile.limits.entries {
        return Err("requested entry budget exceeded".into());
    }
    let mut items = Vec::new();
    let mut unique = BTreeMap::new();
    let mut destination_bytes = 0_u64;
    let mut destinations = BTreeSet::<String>::new();
    let mut checkpoint = None;
    let mut row_hash = Sha256::new();
    row_hash.update(b"tig.project-restore-entries.experimental-1\0");
    for entry in entries {
        let dest = entry.destination.as_str();
        if !manifest
            .destination_prefixes
            .iter()
            .any(|p| dest.starts_with(&(p.as_str().to_owned() + "/")))
        {
            return Err("destination outside declared prefix".into());
        }
        let folded = dest.to_lowercase();
        if destinations.iter().any(|old| {
            old == &folded
                || old.starts_with(&(folded.clone() + "/"))
                || folded.starts_with(&(old.clone() + "/"))
        }) {
            return Err("destination alias or parent collision".into());
        }
        destinations.insert(folded);
        let asset = assets
            .resolve_id(&entry.id)
            .map_err(|_| "requested asset unavailable")?;
        checkpoint.get_or_insert_with(|| asset.checkpoint().clone());
        let object = select(&asset, &entry.selector)?;
        if unique
            .insert(object.sha256.clone(), object.bytes)
            .is_some_and(|size| size != object.bytes)
        {
            return Err("inconsistent object length".into());
        }
        destination_bytes = destination_bytes
            .checked_add(object.bytes)
            .ok_or("destination byte overflow")?;
        if destination_bytes > profile.limits.destination_bytes {
            return Err("destination byte budget exceeded".into());
        }
        let item = Item {
            record: asset.reference().clone(),
            definition: asset.definition().clone(),
            selector: entry.selector,
            object,
            destination: entry.destination,
        };
        let raw =
            canonical_json(&serde_json::to_value(&item).map_err(|_| "invalid planned entry")?)
                .map_err(|_| "invalid planned entry")?;
        row_hash.update((raw.len() as u64).to_le_bytes());
        row_hash.update(raw);
        items.push(item);
    }
    manifest.requirements.validate(
        &profile.host_files,
        profile.limits.host_verification_bytes,
        profile.capacity_root.as_deref(),
        destination_bytes,
    )?;
    let checkpoint = checkpoint.ok_or("empty source selection")?;
    let entry_digest: String = row_hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let profile_object = Object::from_bytes(raw_profile);
    let manifest_object = Object::from_bytes(&raw_manifest);
    let identity = Object::from_bytes(&canonical_json(&serde_json::json!({"schema":"tig.project-restore-plan.experimental-1",
        "profile":profile_object,"manifest":manifest_object,"manifest_path":manifest_path,"checkpoint":checkpoint,"entries":entry_digest}))
        .map_err(|_| "plan identity budget exceeded")?).sha256;
    Ok(Plan {
        identity,
        source_commit: profile.commit.clone(),
        checkpoint,
        profile_object,
        manifest_object,
        manifest_path,
        profile,
        items,
        unique,
        destination_bytes,
        metadata_millis: elapsed(start),
        requirements: manifest.requirements,
    })
}

impl Plan {
    pub fn identity(&self) -> &Digest {
        &self.identity
    }

    pub fn run(&self, apply: bool, expected: Option<&Digest>) -> Result<Report> {
        self.run_observed(apply, expected, |_| true)
    }

    /// An explicit trusted consumer may persist progress or stop between entries.
    /// This callback is not loaded from a manifest and creates no durable journal.
    pub fn run_observed(
        &self,
        apply: bool,
        expected: Option<&Digest>,
        mut observe: impl FnMut(&EntryResult) -> bool,
    ) -> Result<Report> {
        if expected.is_some_and(|old| old != &self.identity) {
            return Err("expected plan differs".into());
        }
        let unique_bytes = self
            .unique
            .values()
            .try_fold(0_u64, |sum, n| sum.checked_add(*n))
            .ok_or("source byte overflow")?;
        let required_reads = self.unique.len() + if apply { self.items.len() } else { 0 };
        let required_bytes = unique_bytes
            .checked_add(if apply { self.destination_bytes } else { 0 })
            .ok_or("source byte overflow")?;
        if required_reads > self.profile.byte_source.limits.requests as usize
            || required_bytes > self.profile.byte_source.limits.total_bytes
            || self
                .unique
                .values()
                .any(|n| *n > self.profile.byte_source.limits.object_bytes)
        {
            return Err("source verification budget insufficient for complete operation".into());
        }
        let mut cache =
            ReadCache::new(self.profile.byte_source.clone()).map_err(|e| e.to_string())?;
        let mut report = Report {
            schema: "tig.project-restore-result.experimental-4",
            source_commit: self.source_commit.clone(),
            source_checkpoint: self.checkpoint.clone(),
            profile: self.profile_object.clone(),
            manifest: self.manifest_object.clone(),
            manifest_path: self.manifest_path.clone(),
            plan: self.identity.clone(),
            apply,
            unique_records: self
                .items
                .iter()
                .map(|i| &i.record.id)
                .collect::<BTreeSet<_>>()
                .len(),
            unique_objects: self.unique.len(),
            unique_object_bytes: unique_bytes,
            destination_bytes: self.destination_bytes,
            copied_bytes: 0,
            source_verifications: 0,
            logical_source_verification_bytes: 0,
            preflight_protection_modes: Vec::new(),
            metadata_millis: self.metadata_millis,
            preflight_millis: 0,
            copy_millis: 0,
            entries: self
                .items
                .iter()
                .enumerate()
                .map(|(ordinal, i)| EntryResult {
                    ordinal,
                    record: i.record.clone(),
                    object: i.object.clone(),
                    selector: i.selector.clone(),
                    destination: i.destination.clone(),
                    action: "pending",
                    destination_verified: false,
                    source_protection_observed_at_copy: None,
                })
                .collect(),
            host_files: Vec::new(),
            capacity: None,
            consumer_evidence: self.profile.consumer_evidence.clone(),
            consumer_evidence_verified: false,
            failure: None,
            source_guards_held: false,
            independent_custody_verified: false,
            authorizes_eviction: false,
            complete_semantic_coverage: false,
        };
        let preflight = Instant::now();
        report.host_files = self.requirements.observe_hosts(&self.profile.host_files);
        if report
            .host_files
            .iter()
            .any(|file| !file.verified_at_preflight)
        {
            report.failure = Some(Failure {
                ordinal: None,
                phase: "preflight-host",
                reason: "declared host requirements are not verified".into(),
            });
            report.preflight_millis = elapsed(preflight);
            return Ok(report);
        }
        for (ordinal, item) in self.items.iter().enumerate() {
            match restore::check_destination(
                &self
                    .profile
                    .destination_root
                    .join(item.destination.as_str()),
                &item.object,
                true,
            ) {
                Ok(exists) => {
                    report.entries[ordinal].destination_verified = exists;
                    report.entries[ordinal].action =
                        if exists { "already-present" } else { "pending" };
                }
                Err(reason) => {
                    report.entries[ordinal].action = "failed";
                    report.failure = Some(Failure {
                        ordinal: Some(ordinal),
                        phase: "preflight-destination",
                        reason,
                    });
                    break;
                }
            }
        }
        if report.failure.is_none() {
            let new_bytes = report
                .entries
                .iter()
                .filter(|entry| !entry.destination_verified)
                .map(|entry| entry.object.bytes)
                .sum();
            report.capacity = self.requirements.observe_capacity(
                self.profile.capacity_root.as_deref(),
                &self.profile.destination_root,
                new_bytes,
            )?;
            if report
                .capacity
                .as_ref()
                .is_some_and(|space| space.sufficient_for_logical_restore != Some(true))
            {
                report.failure = Some(Failure {
                    ordinal: None,
                    phase: "preflight-capacity",
                    reason: "declared space threshold is not observed satisfied".into(),
                });
            }
        }
        if report.failure.is_none() {
            for (hash, size) in &self.unique {
                match cache.open_verified(&Object {
                    sha256: hash.clone(),
                    bytes: *size,
                }) {
                    Ok(reader) => {
                        if !report
                            .preflight_protection_modes
                            .contains(&reader.protection())
                        {
                            report.preflight_protection_modes.push(reader.protection());
                        }
                    }
                    Err(reason) => {
                        let ordinal = self.items.iter().position(|i| &i.object.sha256 == hash);
                        if let Some(n) = ordinal {
                            report.entries[n].action = "failed";
                        }
                        report.failure = Some(Failure {
                            ordinal,
                            phase: "preflight-source",
                            reason: reason.to_string(),
                        });
                        break;
                    }
                }
            }
        }
        report.preflight_millis = elapsed(preflight);
        if report.failure.is_none() {
            let copy = Instant::now();
            for (ordinal, item) in self.items.iter().enumerate() {
                if !apply {
                    if !report.entries[ordinal].destination_verified {
                        report.entries[ordinal].action = "would-restore";
                    }
                    continue;
                }
                let destination = self
                    .profile
                    .destination_root
                    .join(item.destination.as_str());
                let outcome = restore::create_parents(&destination)
                    .and_then(|()| restore::restore(&mut cache, &item.object, &destination, true));
                match outcome {
                    Ok(result) => {
                        report.entries[ordinal].action = result.action;
                        report.entries[ordinal].destination_verified = result.destination_verified;
                        report.entries[ordinal].source_protection_observed_at_copy =
                            Some(result.source_protection_at_copy);
                        report.copied_bytes += result.copied_bytes;
                        if !observe(&report.entries[ordinal]) {
                            report.failure = Some(Failure {
                                ordinal: Some(ordinal),
                                phase: "interrupted",
                                reason: "explicit observer stop".into(),
                            });
                            break;
                        }
                    }
                    Err(reason) => {
                        report.entries[ordinal].action = "failed";
                        report.failure = Some(Failure {
                            ordinal: Some(ordinal),
                            phase: "copy",
                            reason,
                        });
                        break;
                    }
                }
            }
            report.copy_millis = elapsed(copy);
        }
        report.source_verifications = cache.requests();
        report.logical_source_verification_bytes = cache.verification_bytes();
        Ok(report)
    }
}
