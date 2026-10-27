//! Explicit bounded native catalog from captured Git blobs, never working files.
pub mod archive;
pub mod context;
pub mod job;
pub mod produced;
use super::{Error, Repository, Result, Scope, Snapshot, View};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
use tig_assets::{
    graph::{self, Reference},
    native::{Instance, MAX_RECORD_BYTES},
    resolve::{AssetResolver, Error as ResolveError, Resolved},
};
use tig_core::{Digest, LogicalPath, Object, identity::canonical_json};

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub documents: usize,
    pub bytes: usize,
    pub millis: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            documents: 16_384,
            bytes: 16 * 1024 * 1024,
            millis: 30_000,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<()> {
        if !(1..=65_536).contains(&self.documents)
            || !(1..=128 * 1024 * 1024).contains(&self.bytes)
            || !(1..=300_000).contains(&self.millis)
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
}

struct Record {
    definition: Object,
    schema: Object,
    resolved: Resolved,
}

pub struct AssetSnapshot {
    checkpoint: Digest,
    selected: BTreeMap<String, Reference>,
    records: BTreeMap<Reference, Record>,
    blobs: BTreeMap<Object, Vec<u8>>,
    limits: Limits,
}
impl AssetSnapshot {
    pub fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub fn len(&self) -> usize {
        self.selected.len()
    }
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
    pub fn metadata_bytes(&self) -> usize {
        self.blobs.values().map(Vec::len).sum()
    }

    /// Explicit old snapshots supply immutable metadata only, never ID selections.
    /// This is memory lifetime/recovery, not a durable retention acknowledgement.
    pub fn retain(&mut self, other: Self) -> Result<()> {
        let extra: usize = other
            .blobs
            .iter()
            .filter(|(o, _)| !self.blobs.contains_key(*o))
            .map(|(_, b)| b.len())
            .sum();
        let records = other
            .records
            .keys()
            .filter(|r| !self.records.contains_key(*r))
            .count();
        let objects = other
            .blobs
            .keys()
            .filter(|o| !self.blobs.contains_key(*o))
            .count();
        if self
            .metadata_bytes()
            .checked_add(extra)
            .is_none_or(|n| n > self.limits.bytes)
            || self
                .records
                .len()
                .checked_add(records)
                .is_none_or(|n| n > self.limits.documents)
            || self
                .blobs
                .len()
                .checked_add(objects)
                .is_none_or(|n| n > self.limits.documents)
        {
            return Err(Error::Budget);
        }
        for (object, bytes) in other.blobs {
            self.blobs.entry(object).or_insert(bytes);
        }
        for (reference, record) in other.records {
            self.records.entry(reference).or_insert(record);
        }
        Ok(())
    }
    fn bind_records(
        &mut self,
        native: Vec<super::GitId>,
        by_git: &BTreeMap<super::GitId, Object>,
        start: Instant,
    ) -> Result<()> {
        for id in native {
            if start.elapsed() > Duration::from_millis(self.limits.millis) {
                return Err(Error::Budget);
            }
            let object = by_git.get(&id).ok_or(Error::Integrity)?;
            let parsed = Instance::parse(self.blobs.get(object).ok_or(Error::Integrity)?)
                .map_err(|_| Error::Integrity)?;
            let definition = parsed.value().definition.object();
            let def = self.blobs.get(&definition).ok_or(Error::Incomplete)?;
            let schema = tig_assets::native::Definition::parse(def)
                .map_err(|_| Error::Integrity)?
                .value()
                .authored_schema
                .object();
            let bound = parsed
                .bind(def, self.blobs.get(&schema).ok_or(Error::Incomplete)?)
                .map_err(|_| Error::Integrity)?;
            let resolved = Resolved::from_bound(self.checkpoint.clone(), &bound);
            let reference = resolved.reference().clone();
            if self
                .selected
                .insert(reference.id.clone(), reference.clone())
                .is_some()
            {
                return Err(Error::Ambiguous);
            }
            self.records.insert(
                reference,
                Record {
                    definition,
                    schema,
                    resolved,
                },
            );
        }
        Ok(())
    }
}

impl AssetResolver for AssetSnapshot {
    fn resolve_id(&self, id: &str) -> std::result::Result<Resolved, ResolveError> {
        let reference = self.selected.get(id).ok_or(ResolveError::Missing)?;
        self.records
            .get(reference)
            .map(|r| r.resolved.clone())
            .ok_or(ResolveError::Missing)
    }
}
impl graph::Source for AssetSnapshot {
    fn select(&mut self, id: &str, remaining: usize) -> graph::SelectionLookup {
        let selection = match self.selected.get(id) {
            Some(reference)
                if serde_json::to_vec(reference).map_or(true, |b| b.len() > remaining) =>
            {
                graph::Selection::Unavailable
            }
            Some(reference) => graph::Selection::Found(reference.clone()),
            None => graph::Selection::Missing,
        };
        graph::SelectionLookup {
            checkpoint: self.checkpoint.clone(),
            selection,
        }
    }
    fn lookup(&mut self, reference: &Reference, remaining: usize) -> graph::Lookup {
        let data = match self.records.get(reference) {
            Some(record) => {
                let objects = [&reference.object, &record.definition, &record.schema];
                let total = objects
                    .iter()
                    .try_fold(0usize, |n, o| n.checked_add(o.bytes as usize));
                if total.is_none_or(|n| n > remaining) {
                    graph::Data::Unavailable
                } else if let (Some(i), Some(d), Some(s)) = (
                    self.blobs.get(objects[0]),
                    self.blobs.get(objects[1]),
                    self.blobs.get(objects[2]),
                ) {
                    graph::Data::Record {
                        instance: i.clone(),
                        definition: d.clone(),
                        schema: s.clone(),
                    }
                } else {
                    graph::Data::Missing
                }
            }
            None => graph::Data::Missing,
        };
        graph::Lookup {
            checkpoint: self.checkpoint.clone(),
            data,
        }
    }
}

impl Repository {
    pub fn native(
        &self,
        snapshot: &Snapshot,
        records: Scope,
        definitions: Scope,
        limits: Limits,
    ) -> Result<AssetSnapshot> {
        limits.validate()?;
        if snapshot.enrollment != self.enrollment {
            return Err(Error::Invalid);
        }
        let start = Instant::now();
        let mut ids = BTreeSet::new();
        let mut native = Vec::new();
        let mut documents = 0;
        for entry in &snapshot.entries {
            let record = records.contains(&entry.path) && entry.path.ends_with(".tig");
            let support = definitions.contains(&entry.path) && entry.path.ends_with(".json");
            if !record && !support {
                continue;
            }
            documents += 1;
            if documents > limits.documents {
                return Err(Error::Budget);
            }
            if entry.stage != 0 || !matches!(entry.mode.as_str(), "100644" | "100755") {
                return Err(Error::Incomplete);
            }
            LogicalPath::parse(&entry.path).map_err(|_| Error::Invalid)?;
            ids.insert(entry.oid.clone());
            if record {
                native.push(entry.oid.clone());
            }
        }
        let checkpoint = checkpoint(snapshot, &records, &definitions)?;
        let mut result = AssetSnapshot {
            checkpoint,
            selected: BTreeMap::new(),
            records: BTreeMap::new(),
            blobs: BTreeMap::new(),
            limits,
        };
        let mut by_git = BTreeMap::new();
        let ids: Vec<_> = ids.into_iter().collect();
        for chunk in ids.chunks(self.limits.pointer_blobs.min(256)) {
            if start.elapsed() > Duration::from_millis(limits.millis) {
                return Err(Error::Budget);
            }
            let remaining = limits.bytes - result.metadata_bytes();
            let blobs = self.read_blobs(
                chunk.iter().cloned().collect(),
                MAX_RECORD_BYTES,
                remaining,
                true,
            )?;
            for (id, bytes) in blobs {
                let bytes = bytes.ok_or(Error::Incomplete)?;
                if bytes.is_empty() {
                    return Err(Error::Incomplete);
                }
                let object = Object::from_bytes(&bytes);
                by_git.insert(id, object.clone());
                result.blobs.insert(object, bytes);
            }
        }
        result.bind_records(native, &by_git, start)?;
        if start.elapsed() > Duration::from_millis(limits.millis) {
            return Err(Error::Budget);
        }
        Ok(result)
    }
}

fn checkpoint(snapshot: &Snapshot, records: &Scope, definitions: &Scope) -> Result<Digest> {
    let view = match &snapshot.checkpoint.view {
        View::Index => serde_json::json!({"kind":"index"}),
        View::Commit(id) => serde_json::json!({"kind":"commit","oid":id.as_str()}),
    };
    let json = serde_json::json!({"schema":"tig.native-git-snapshot.experimental-1",
        "view":view,"listing":snapshot.checkpoint.listing,
        "records":records.prefixes().map(|p| p.as_str()).collect::<Vec<_>>(),
        "definitions":definitions.prefixes().map(|p| p.as_str()).collect::<Vec<_>>()});
    canonical_json(&json)
        .map(|b| Digest::blob(&b))
        .map_err(|_| Error::Budget)
}
