//! Positive-only collector facts for current admitted inventory INDEX metadata.
//! Inventory members are observations, never active media retention roots.
use super::*;

#[derive(Clone, Copy)]
pub struct Limits {
    pub keys: usize,
    pub metadata_bytes: u64,
    pub node_bytes: u64,
    pub total_bytes: u64,
    pub nodes: usize,
    pub objects: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            keys: 10_000,
            metadata_bytes: 64 * 1024 * 1024,
            node_bytes: 256 * 1024,
            total_bytes: 1024 * 1024 * 1024,
            nodes: 100_000,
            objects: 1_000_000,
        }
    }
}
pub enum Membership {
    ProtectedMetadata,
    OutsideCatalogScope,
}
pub struct ProtectionSnapshot {
    stamp: BTreeMap<String, Blob>,
    catalog: Blob,
    protected: BTreeMap<String, u64>,
    sources: BTreeMap<String, u64>,
    limits: Limits,
}
impl ProtectionSnapshot {
    pub fn protected_nodes(&self) -> usize {
        self.protected.len()
    }
    pub fn required_sources(&self) -> usize {
        self.sources.len()
    }
    pub fn keys(&self) -> usize {
        self.stamp.len()
    }
    pub fn classify(&self, subject: &str) -> Result<Membership> {
        if !valid_oid(subject) {
            return Err("Invalid exact metadata protection subject".into());
        }
        Ok(if self.protected.contains_key(subject) {
            Membership::ProtectedMetadata
        } else {
            Membership::OutsideCatalogScope
        })
    }
    /// Required original ENCODED evidence, not a claim of verified local custody.
    pub fn requires_original_source(&self, subject: &str) -> Result<bool> {
        if !valid_oid(subject) {
            return Err("Invalid exact provenance subject".into());
        }
        Ok(self.sources.contains_key(subject))
    }
    /// Fresh full verification plus revision/membership comparison. This is not
    /// a removal lease and cannot fence a later writer after the call returns.
    pub fn revalidate(&self, catalog: &Catalog) -> Result<()> {
        let current = collect(catalog, self.limits)?;
        if current.catalog != self.catalog
            || current.stamp != self.stamp
            || current.protected != self.protected
            || current.sources != self.sources
        {
            return Err("Catalog protection snapshot is stale".into());
        }
        Ok(())
    }
}
fn states(
    catalog: &Catalog,
    max_keys: usize,
    meter: &mut ControlMeter,
) -> Result<BTreeMap<String, Snapshot>> {
    meter.charge(catalog.identity.bytes)?;
    catalog.check_manifest()?;
    checked_directory(&catalog.directory.join("members"))?;
    let mut snapshots = BTreeMap::new();
    let mut entries = 0usize;
    for entry in fs::read_dir(catalog.directory.join("members"))? {
        entries = entries
            .checked_add(1)
            .ok_or("Catalog membership count overflow")?;
        if entries > max_keys.saturating_mul(2) {
            return Err("Catalog membership enumeration budget exceeded".into());
        }
        let entry = entry?;
        plain(&entry.path(), false)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Invalid catalog member filename")?;
        let (stem, extension) = name
            .rsplit_once('.')
            .ok_or("Unrecognized catalog membership entry")?;
        if !valid_oid(stem) {
            return Err("Invalid catalog membership route".into());
        }
        if extension == "pending" {
            if !exists(&entry.path().with_extension("json"))? {
                return Err("Incomplete catalog membership publication".into());
            }
            continue;
        }
        if extension != "json" || snapshots.len() >= max_keys {
            return Err("Catalog membership scope/budget rejected".into());
        }
        let member: Member = meter.read(&entry.path())?;
        if member.schema != MEMBER
            || member.catalog != catalog.tag()
            || hash(member.key.as_bytes()) != stem
        {
            return Err("Catalog member scope/routing mismatch".into());
        }
        let snapshot = catalog.read_with(&member.key, meter)?;
        if snapshots.insert(member.key, snapshot).is_some() {
            return Err("Duplicate catalog membership".into());
        }
    }
    let expected: BTreeSet<_> = snapshots.keys().map(|key| hash(key.as_bytes())).collect();
    let mut actual = BTreeSet::new();
    checked_directory(&catalog.directory.join("keys"))?;
    for (count, entry) in fs::read_dir(catalog.directory.join("keys"))?.enumerate() {
        if count >= max_keys {
            return Err("Catalog slot enumeration budget exceeded".into());
        }
        let entry = entry?;
        plain(&entry.path(), true)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Invalid catalog slot filename")?;
        if !valid_oid(&name) {
            return Err("Unknown catalog slot scope".into());
        }
        actual.insert(name);
    }
    if actual != expected {
        return Err("Incomplete catalog membership/slot coverage".into());
    }
    Ok(snapshots)
}
fn stamp(states: &BTreeMap<String, Snapshot>) -> BTreeMap<String, Blob> {
    states
        .iter()
        .map(|(key, s)| (key.clone(), s.head.clone()))
        .collect()
}
fn insert(map: &mut BTreeMap<String, u64>, blob: &Blob) -> Result<()> {
    if map
        .insert(blob.sha256.clone(), blob.bytes)
        .is_some_and(|bytes| bytes != blob.bytes)
    {
        return Err("Conflicting exact metadata descriptors".into());
    }
    Ok(())
}
/// Double-collects membership/revisions around bounded root verification. Any
/// incomplete/corrupt/busy record is an error, never a partial protection set.
pub fn collect(catalog: &Catalog, limits: Limits) -> Result<ProtectionSnapshot> {
    check_node_limit(limits.node_bytes)?;
    if limits.keys > 100_000
        || limits.metadata_bytes > 256 * 1024 * 1024
        || limits.total_bytes > 4 * 1024 * 1024 * 1024
        || limits.nodes > 1_000_000
        || limits.objects > 1_000_000
    {
        return Err("Catalog projection budget exceeds ceiling".into());
    }
    let mut metadata = ControlMeter {
        used: 0,
        max: limits.metadata_bytes,
    };
    let before = states(catalog, limits.keys, &mut metadata)?;
    let mut protected = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut bytes = 0u64;
    let mut nodes = 0usize;
    let mut objects = 0usize;
    for state in before.values() {
        let Some(binding) = &state.state.binding else {
            continue;
        };
        let source = binding.source()?;
        metadata.charge(binding.receipt.bytes)?;
        let read = ReadLimits {
            node_bytes: limits.node_bytes.min(binding.limits.node_bytes),
            total_bytes: (limits.total_bytes - bytes).min(binding.limits.total_bytes),
            nodes: (limits.nodes - nodes).min(binding.limits.nodes),
            rows: (limits.objects - objects).min(binding.limits.rows),
        };
        let (graph, receipt) = retirement::verify_anchor(
            &binding.cache,
            &binding.retained_parent,
            &binding.root.blob(),
            &source,
            read,
        )?;
        if receipt != binding.receipt.blob() {
            return Err("Catalog retained evidence changed".into());
        }
        bytes = bytes
            .checked_add(graph.bytes_read)
            .ok_or("Protection byte overflow")?;
        nodes = nodes
            .checked_add(graph.nodes_read)
            .ok_or("Protection node overflow")?;
        objects = objects
            .checked_add(graph.objects)
            .ok_or("Protection object overflow")?;
        for blob in &graph.descriptors {
            insert(&mut protected, blob)?;
        }
        insert(&mut sources, &source.encoded)?;
    }
    let after = states(catalog, limits.keys, &mut metadata)?;
    let before = stamp(&before);
    if before != stamp(&after) {
        return Err("Catalog changed during protection verification; retry".into());
    }
    Ok(ProtectionSnapshot {
        stamp: before,
        catalog: catalog.identity.clone(),
        protected,
        sources,
        limits,
    })
}
