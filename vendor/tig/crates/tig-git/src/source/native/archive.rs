//! Exact source interpretation through existing immutable metadata ports.
//! Candidate roots require provider finish/readback and owner adoption separately.
use super::{AssetSnapshot, Error, Limits, Record, Reference, Resolved, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use tig_assets::native::{Instance, MAX_RECORD_BYTES};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeError, TreeLimits},
};

const SCHEMA: &str = "tig.native-source-archive.experimental-1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    checkpoint: Digest,
    tree: Object,
    blobs: usize,
    records: usize,
    selected: usize,
    bytes: usize,
}

fn tree_error(error: TreeError) -> Error {
    match error {
        TreeError::Budget => Error::Budget,
        TreeError::MissingNode(_) => Error::Incomplete,
        _ => Error::Metadata,
    }
}
fn timed(start: Instant, limits: Limits) -> Result<()> {
    if start.elapsed() > Duration::from_millis(limits.millis) {
        Err(Error::Budget)
    } else {
        Ok(())
    }
}
fn path(namespace: &str, digest: &Digest) -> Result<LogicalPath> {
    LogicalPath::parse(&format!("{namespace}/{}", digest.as_str())).map_err(|_| Error::Invalid)
}
fn put(store: &mut impl NodeStore, bytes: &[u8]) -> Result<Object> {
    let expected = Object::from_bytes(bytes);
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(Error::Budget);
    }
    let actual = store.put(bytes).map_err(tree_error)?;
    if actual != expected {
        return Err(Error::Integrity);
    }
    Ok(actual)
}
fn read(store: &impl NodeStore, object: &Object, remaining: usize) -> Result<Vec<u8>> {
    if object.bytes > remaining.min(MAX_RECORD_BYTES) as u64 {
        return Err(Error::Budget);
    }
    let bytes = store
        .get(object)
        .map_err(tree_error)?
        .ok_or(Error::Incomplete)?;
    if bytes.len() > remaining.min(MAX_RECORD_BYTES) {
        return Err(Error::Budget);
    }
    if Object::from_bytes(&bytes) != *object {
        return Err(Error::Integrity);
    }
    Ok(bytes)
}

impl AssetSnapshot {
    /// Write a candidate metadata archive, not a durable retention acknowledgement.
    /// No selected payload/stream bytes are read. Old archives are never modified.
    pub fn archive_metadata(
        &self,
        store: &mut impl NodeStore,
        tree_limits: TreeLimits,
    ) -> Result<Object> {
        self.limits.validate()?;
        let start = Instant::now();
        let count = self
            .blobs
            .len()
            .checked_add(self.records.len())
            .and_then(|n| n.checked_add(self.selected.len()))
            .ok_or(Error::Budget)?;
        if count > tree_limits.max_entries {
            return Err(Error::Budget);
        }
        let mut entries = BTreeMap::new();
        for (object, bytes) in &self.blobs {
            timed(start, self.limits)?;
            if put(store, bytes)? != *object {
                return Err(Error::Integrity);
            }
            entries.insert(path("blobs", &object.sha256)?, object.clone());
        }
        for reference in self.records.keys() {
            entries.insert(
                path("records", &reference.object.sha256)?,
                reference.object.clone(),
            );
        }
        for (id, reference) in &self.selected {
            entries.insert(
                path("selected", &Digest::blob(id.as_bytes()))?,
                reference.object.clone(),
            );
        }
        let (tree, _) = Tree::build(&entries, store, tree_limits).map_err(tree_error)?;
        timed(start, self.limits)?;
        let header = Header {
            schema: SCHEMA.into(),
            checkpoint: self.checkpoint.clone(),
            tree: tree.root().clone(),
            blobs: self.blobs.len(),
            records: self.records.len(),
            selected: self.selected.len(),
            bytes: self.metadata_bytes(),
        };
        let bytes =
            identity::canonical_json(&serde_json::to_value(header).map_err(|_| Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        let root = put(store, &bytes)?;
        timed(start, self.limits)?;
        Ok(root)
    }

    /// Restore an explicitly admitted exact source view without Git or a worktree.
    /// Provider closure verification is not source enrollment or payload custody.
    pub fn from_metadata_archive(
        store: &mut impl NodeStore,
        root: &Object,
        limits: Limits,
        tree_limits: TreeLimits,
    ) -> Result<Self> {
        limits.validate()?;
        let start = Instant::now();
        let bytes = read(store, root, MAX_RECORD_BYTES)?;
        let value = identity::parse_json(&bytes).map_err(|_| Error::Integrity)?;
        let header: Header = serde_json::from_value(value).map_err(|_| Error::Integrity)?;
        if header.schema != SCHEMA {
            return Err(Error::Invalid);
        }
        if header.blobs > limits.documents
            || header.records > limits.documents
            || header.selected > header.records
            || header.bytes > limits.bytes
        {
            return Err(Error::Budget);
        }
        let (entries, _) = Tree::from_root(header.tree)
            .entries(store, tree_limits)
            .map_err(tree_error)?;
        timed(start, limits)?;
        let expected = header
            .blobs
            .checked_add(header.records)
            .and_then(|n| n.checked_add(header.selected))
            .ok_or(Error::Budget)?;
        if entries.len() != expected {
            return Err(Error::Integrity);
        }
        let mut snapshot = Self {
            checkpoint: header.checkpoint,
            selected: BTreeMap::new(),
            records: BTreeMap::new(),
            blobs: BTreeMap::new(),
            limits,
        };
        let mut records = Vec::new();
        let mut selected = Vec::new();
        let mut total = 0usize;
        for (key, object) in entries {
            timed(start, limits)?;
            let parts = key.components().collect::<Vec<_>>();
            if parts.len() != 2 {
                return Err(Error::Integrity);
            }
            match parts[0] {
                "blobs" => {
                    if parts[1] != object.sha256.as_str() || snapshot.blobs.len() == header.blobs {
                        return Err(Error::Integrity);
                    }
                    let bytes = read(store, &object, limits.bytes.saturating_sub(total))?;
                    total = total.checked_add(bytes.len()).ok_or(Error::Budget)?;
                    snapshot.blobs.insert(object, bytes);
                }
                "records"
                    if parts[1] == object.sha256.as_str() && records.len() < header.records =>
                {
                    records.push(object)
                }
                "selected" if selected.len() < header.selected => {
                    selected.push((parts[1].to_owned(), object))
                }
                _ => return Err(Error::Integrity),
            }
        }
        if snapshot.blobs.len() != header.blobs
            || records.len() != header.records
            || selected.len() != header.selected
            || total != header.bytes
        {
            return Err(Error::Integrity);
        }
        for object in records {
            timed(start, limits)?;
            let parsed = Instance::parse(snapshot.blobs.get(&object).ok_or(Error::Incomplete)?)
                .map_err(|_| Error::Integrity)?;
            let definition = parsed.value().definition.object();
            let def = snapshot.blobs.get(&definition).ok_or(Error::Incomplete)?;
            let schema = tig_assets::native::Definition::parse(def)
                .map_err(|_| Error::Integrity)?
                .value()
                .authored_schema
                .object();
            let bound = parsed
                .bind(def, snapshot.blobs.get(&schema).ok_or(Error::Incomplete)?)
                .map_err(|_| Error::Integrity)?;
            let resolved = Resolved::from_bound(snapshot.checkpoint.clone(), &bound);
            let reference = resolved.reference().clone();
            snapshot.records.insert(
                reference,
                Record {
                    definition,
                    schema,
                    resolved,
                },
            );
        }
        for (key, object) in selected {
            timed(start, limits)?;
            let parsed = Instance::parse(snapshot.blobs.get(&object).ok_or(Error::Incomplete)?)
                .map_err(|_| Error::Integrity)?;
            let id = parsed.value().id.clone();
            let reference = Reference {
                id: id.clone(),
                object,
            };
            if key != Digest::blob(id.as_bytes()).as_str()
                || !snapshot.records.contains_key(&reference)
                || snapshot.selected.insert(id, reference).is_some()
            {
                return Err(Error::Integrity);
            }
        }
        timed(start, limits)?;
        Ok(snapshot)
    }
}
