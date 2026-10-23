//! Opt-in, read-only adapter for existing cache-report metadata.
//!
//! Does not resolve paths, probe storage, select roots, publish indexes or authorize
//! movement. Source descriptors/authority must be admitted by the caller. The
//! prepared index is in memory; no sharded interchange format is introduced.
use crate::{Result, valid_oid};
use serde::{
    Deserialize, Serialize,
    de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::Read,
    marker::PhantomData,
};

pub const ADAPTER_ID: &str = "reel.cache-report-metadata";
pub const ADAPTER_VERSION: u32 = 1;
pub const SOURCE_SCHEMA: &str = "reel.asset-cache-report.v1";
const MAX_TIERS: usize = 64;

#[derive(Clone, PartialEq, Eq)]
pub struct Blob {
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Copy)]
pub enum Encoding {
    Identity,
    Gzip,
}
/// Private source identity and externally admitted authority, not self-attestation.
#[derive(Clone)]
pub struct Source {
    pub encoded: Blob,
    pub decoded: Blob,
    pub encoding: Encoding,
    pub schema: String,
    pub project_id: String,
    pub authority_namespace: String,
    pub adapter_version: u32,
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub encoded_bytes: u64,
    pub decoded_bytes: u64,
    pub objects: usize,
    pub total_copies: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            encoded_bytes: 256 * 1024 * 1024,
            decoded_bytes: 512 * 1024 * 1024,
            objects: 1_000_000,
            total_copies: 2_000_000,
        }
    }
}
/// Private captured location metadata. A volume ID is not independent-custody proof.
#[derive(Deserialize)]
pub struct TierObservation {
    pub tier: usize,
    pub root: String,
    #[serde(default)]
    pub volume_id: Option<String>,
}
/// Metadata observation only. Intentionally has no payload-verified/custody flag.
#[derive(Deserialize)]
pub struct CopyObservation {
    pub tier: usize,
    pub bytes: u64,
    #[serde(default)]
    pub physical_id: Option<String>,
    #[serde(default)]
    pub physical_identity_measured: Option<bool>,
    #[serde(default)]
    pub allocated_bytes: Option<u64>,
    #[serde(default)]
    pub hard_links: Option<u64>,
    #[serde(default)]
    pub known_lfs_hard_links: Option<u64>,
    #[serde(default)]
    pub other_hard_links: Option<u64>,
    #[serde(default)]
    pub descriptor_size_matches: Option<bool>,
}
#[derive(Deserialize)]
pub struct ObservedObject {
    pub sha256: String,
    /// Declared size if registered; otherwise a producer observation summary,
    /// which need not equal every unregistered copy's observed size.
    pub bytes: Option<u64>,
    pub registered: bool,
    #[serde(deserialize_with = "copies")]
    pub copies: Vec<CopyObservation>,
    #[serde(skip)]
    pub source_record: usize,
}
fn copies<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<CopyObservation>, D::Error> {
    Bounded::<CopyObservation>::new(MAX_TIERS).deserialize(d)
}
struct Bounded<T> {
    limit: usize,
    cost_limit: usize,
    cost: fn(&T) -> usize,
    marker: PhantomData<T>,
}
impl<T> Bounded<T> {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            cost_limit: usize::MAX,
            cost: |_| 0,
            marker: PhantomData,
        }
    }
    fn with_cost(mut self, limit: usize, cost: fn(&T) -> usize) -> Self {
        self.cost_limit = limit;
        self.cost = cost;
        self
    }
}
struct Item<T> {
    allowed: bool,
    marker: PhantomData<T>,
}
impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for Item<T> {
    type Value = T;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<T, D::Error> {
        if !self.allowed {
            return Err(de::Error::custom(
                "Record budget exceeded before next record decode",
            ));
        }
        T::deserialize(d)
    }
}
impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for Bounded<T> {
    type Value = Vec<T>;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        d: D,
    ) -> std::result::Result<Vec<T>, D::Error> {
        struct V<T> {
            limit: usize,
            cost_limit: usize,
            cost: fn(&T) -> usize,
            marker: PhantomData<T>,
        }
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = Vec<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("bounded record array")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Vec<T>, A::Error> {
                let mut rows = Vec::new();
                let mut cost = 0usize;
                while let Some(row) = a.next_element_seed(Item::<T> {
                    allowed: rows.len() < self.limit,
                    marker: PhantomData,
                })? {
                    cost = cost
                        .checked_add((self.cost)(&row))
                        .ok_or_else(|| de::Error::custom("Nested record count overflow"))?;
                    if cost > self.cost_limit {
                        return Err(de::Error::custom(
                            "Inventory copy budget exceeded during decode",
                        ));
                    }
                    rows.push(row);
                }
                Ok(rows)
            }
        }
        d.deserialize_seq(V::<T> {
            limit: self.limit,
            cost_limit: self.cost_limit,
            cost: self.cost,
            marker: PhantomData,
        })
    }
}
#[derive(Default)]
struct Report {
    schema: Option<String>,
    project: Option<String>,
    profile: Option<String>,
    generation: Option<u64>,
    scope: Option<String>,
    named_scope: Option<String>,
    dependency_coverage: Option<bool>,
    started: Option<u64>,
    completed: Option<u64>,
    tiers: Option<Vec<TierObservation>>,
    rows: Option<Vec<ObservedObject>>,
}
struct ReportSeed {
    max_objects: usize,
    max_copies: usize,
}
impl<'de> DeserializeSeed<'de> for ReportSeed {
    type Value = Report;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        d: D,
    ) -> std::result::Result<Report, D::Error> {
        struct V {
            max_objects: usize,
            max_copies: usize,
        }
        impl<'de> Visitor<'de> for V {
            type Value = Report;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("cache report object")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Report, A::Error> {
                let mut r = Report::default();
                let mut seen = BTreeSet::new();
                while let Some(key) = a.next_key::<String>()? {
                    if seen.len() >= 128 || !seen.insert(key.clone()) {
                        return Err(de::Error::custom(
                            "Duplicate field or report field budget exceeded",
                        ));
                    }
                    match key.as_str() {
                        "schema" => r.schema = Some(a.next_value()?),
                        "project_id" => r.project = Some(a.next_value()?),
                        "profile_sha256" => r.profile = Some(a.next_value()?),
                        "registry_generation" => r.generation = Some(a.next_value()?),
                        "scope" => r.scope = Some(a.next_value()?),
                        "inventory_scope" => r.named_scope = Some(a.next_value()?),
                        "coverage_complete" => r.dependency_coverage = Some(a.next_value()?),
                        "census_started_unix_seconds" => r.started = Some(a.next_value()?),
                        "census_completed_unix_seconds" => r.completed = Some(a.next_value()?),
                        "tiers" => {
                            r.tiers =
                                Some(a.next_value_seed(Bounded::<TierObservation>::new(MAX_TIERS))?)
                        }
                        "objects" => {
                            r.rows = Some(
                                a.next_value_seed(
                                    Bounded::<ObservedObject>::new(self.max_objects)
                                        .with_cost(self.max_copies, |row| row.copies.len()),
                                )?,
                            )
                        }
                        _ => {
                            a.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(r)
            }
        }
        d.deserialize_map(V {
            max_objects: self.max_objects,
            max_copies: self.max_copies,
        })
    }
}
/// Capture time is optional for named observations; never replaced by current time.
#[derive(Clone, Copy)]
pub struct Capture {
    pub started: u64,
    pub completed: u64,
}
/// This namespace has only captured membership, never current physical absence.
pub enum Lookup<'a> {
    Listed(&'a ObservedObject),
    NotListedAtCapture,
}
/// Safe aggregate projection; private source/subject/location data is not serialized.
#[derive(Serialize)]
pub struct Summary {
    pub adapter: &'static str,
    pub objects: usize,
    pub copies: usize,
}
/// A locator is provenance into an integrity-checked source, not an inclusion proof.
pub struct Witness<'a> {
    pub source: &'a Source,
    pub json_pointer: String,
}
pub struct PreparedInventory {
    source: Source,
    rows: BTreeMap<String, ObservedObject>,
    tiers: BTreeMap<usize, TierObservation>,
    capture: Option<Capture>,
    profile: String,
    generation: u64,
    scope: Option<String>,
    named_scope: Option<String>,
    dependency_coverage: Option<bool>,
    copy_count: usize,
}
impl PreparedInventory {
    pub fn source(&self) -> &Source {
        &self.source
    }
    pub fn capture(&self) -> Option<Capture> {
        self.capture
    }
    pub fn profile_sha256(&self) -> &str {
        &self.profile
    }
    pub fn registry_generation(&self) -> u64 {
        self.generation
    }
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }
    pub fn named_scope(&self) -> Option<&str> {
        self.named_scope.as_deref()
    }
    /// Producer's dependency-coverage flag, NOT complete physical inventory coverage.
    pub fn producer_dependency_coverage(&self) -> Option<bool> {
        self.dependency_coverage
    }
    pub fn tier(&self, id: usize) -> Option<&TierObservation> {
        self.tiers.get(&id)
    }
    pub fn summary(&self) -> Summary {
        Summary {
            adapter: ADAPTER_ID,
            objects: self.rows.len(),
            copies: self.copy_count,
        }
    }
    pub fn lookup(&self, subject: &str) -> Result<Lookup<'_>> {
        if !valid_oid(subject) {
            return Err("Invalid exact inventory subject".into());
        }
        Ok(self
            .rows
            .get(subject)
            .map_or(Lookup::NotListedAtCapture, Lookup::Listed))
    }
    pub fn witness(&self, subject: &str) -> Result<Option<Witness<'_>>> {
        Ok(match self.lookup(subject)? {
            Lookup::Listed(row) => Some(Witness {
                source: &self.source,
                json_pointer: format!("/objects/{}", row.source_record),
            }),
            Lookup::NotListedAtCapture => None,
        })
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn check_blob(bytes: &[u8], expected: &Blob) -> Result<()> {
    if bytes.len() as u64 != expected.bytes || hash(bytes) != expected.sha256 {
        return Err("Inventory source hash/byte mismatch".into());
    }
    Ok(())
}
fn read_limited(mut reader: impl Read, max: u64) -> Result<Vec<u8>> {
    let bound = max.checked_add(1).ok_or("Invalid byte budget")?;
    let mut bytes = Vec::new();
    reader.by_ref().take(bound).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err("Inventory byte budget exceeded".into());
    }
    Ok(bytes)
}
/// Explicit preparation only. Encoded/decompressed bytes and parsed metadata are
/// bounded but resident in memory. This is not persistent/sharded sidecar indexing.
pub fn prepare(reader: impl Read, source: &Source, limits: Limits) -> Result<PreparedInventory> {
    if source.schema != SOURCE_SCHEMA
        || source.adapter_version != ADAPTER_VERSION
        || !valid_oid(&source.encoded.sha256)
        || !valid_oid(&source.decoded.sha256)
        || source.project_id.trim().is_empty()
        || source.authority_namespace.trim().is_empty()
        || source.project_id.len() > 256
        || source.authority_namespace.len() > 256
    {
        return Err("Unsupported inventory adapter/schema or invalid admitted source".into());
    }
    if limits.encoded_bytes > 512 * 1024 * 1024
        || limits.decoded_bytes > 1024 * 1024 * 1024
        || limits.objects > 1_000_000
        || limits.total_copies > 4_000_000
        || source.encoded.bytes > limits.encoded_bytes
        || source.decoded.bytes > limits.decoded_bytes
    {
        return Err("Inventory preparation budget exceeded".into());
    }
    let encoded = read_limited(reader, source.encoded.bytes)?;
    check_blob(&encoded, &source.encoded)?;
    let decoded = match source.encoding {
        Encoding::Identity => encoded,
        Encoding::Gzip => read_limited(
            flate2::read::MultiGzDecoder::new(encoded.as_slice()),
            source.decoded.bytes,
        )?,
    };
    check_blob(&decoded, &source.decoded)?;
    let mut decoder = serde_json::Deserializer::from_slice(&decoded);
    let report = ReportSeed {
        max_objects: limits.objects,
        max_copies: limits.total_copies,
    }
    .deserialize(&mut decoder)?;
    decoder.end()?;
    if report.schema.as_deref() != Some(SOURCE_SCHEMA)
        || report.project.as_deref() != Some(&source.project_id)
    {
        return Err("Inventory schema/project scope mismatch".into());
    }
    let profile = report.profile.ok_or("Missing profile identity")?;
    if !valid_oid(&profile) {
        return Err("Invalid profile identity".into());
    }
    let generation = report.generation.ok_or("Missing registry generation")?;
    let capture = match (report.started, report.completed) {
        (None, None) => None,
        (Some(started), Some(completed)) if started <= completed => {
            Some(Capture { started, completed })
        }
        _ => return Err("Invalid or partial capture interval".into()),
    };
    let mut tiers = BTreeMap::new();
    for tier in report.tiers.ok_or("Missing tiers")? {
        if tier.root.trim().is_empty() || tiers.contains_key(&tier.tier) {
            return Err("Invalid/duplicate captured tier".into());
        }
        tiers.insert(tier.tier, tier);
    }
    let mut rows = BTreeMap::new();
    let mut copy_count = 0usize;
    for (ordinal, mut row) in report
        .rows
        .ok_or("Missing object records")?
        .into_iter()
        .enumerate()
    {
        if !valid_oid(&row.sha256)
            || (row.registered && row.bytes.is_none())
            || rows.contains_key(&row.sha256)
        {
            return Err("Invalid/duplicate object record".into());
        }
        copy_count = copy_count
            .checked_add(row.copies.len())
            .ok_or("Copy count overflow")?;
        if copy_count > limits.total_copies {
            return Err("Inventory copy budget exceeded".into());
        }
        let mut seen = BTreeSet::new();
        for copy in &row.copies {
            if !tiers.contains_key(&copy.tier)
                || !seen.insert(copy.tier)
                || copy
                    .physical_id
                    .as_ref()
                    .is_some_and(|id| id.trim().is_empty())
                || (copy.physical_identity_measured == Some(false) && copy.physical_id.is_some())
            {
                return Err("Invalid captured copy/tier identity".into());
            }
            if row.registered
                && let (Some(size), Some(matches)) = (row.bytes, copy.descriptor_size_matches)
                && matches != (size == copy.bytes)
            {
                return Err("Inconsistent descriptor-size observation".into());
            }
        }
        row.source_record = ordinal;
        rows.insert(row.sha256.clone(), row);
    }
    Ok(PreparedInventory {
        source: source.clone(),
        rows,
        tiers,
        capture,
        profile,
        generation,
        scope: report.scope,
        named_scope: report.named_scope,
        dependency_coverage: report.dependency_coverage,
        copy_count,
    })
}
