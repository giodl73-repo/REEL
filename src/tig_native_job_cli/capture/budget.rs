//! Transient capture nodes over an existing, read-only metadata provider.
use super::*;
use tig_core::tree::{MemoryNodes, TreeError, TreeResult};
use tig_store::metadata::{Limits, MAX_NODE_BYTES};

struct Overlay<'a, S> {
    existing: &'a S,
    memory: MemoryNodes,
    nodes: u64,
    bytes: u64,
    max_nodes: u64,
    max_bytes: u64,
}
impl<'a, S: NodeStore> Overlay<'a, S> {
    fn new(existing: &'a S, max_nodes: u64, max_bytes: u64) -> Self {
        Self {
            existing,
            memory: MemoryNodes::default(),
            nodes: 0,
            bytes: 0,
            max_nodes,
            max_bytes,
        }
    }
    fn checked(bytes: Option<Vec<u8>>, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        if bytes
            .as_ref()
            .is_some_and(|raw| Object::from_bytes(raw) != *object)
        {
            return Err(TreeError::Integrity);
        }
        Ok(bytes)
    }
}
impl<S: NodeStore> NodeStore for Overlay<'_, S> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        if object.bytes > MAX_NODE_BYTES as u64 {
            return Err(TreeError::Budget);
        }
        if let Some(raw) = Self::checked(self.memory.get(object)?, object)? {
            return Ok(Some(raw));
        }
        Self::checked(self.existing.get(object)?, object)
    }
    fn put(&mut self, raw: &[u8]) -> TreeResult<Object> {
        if raw.len() > MAX_NODE_BYTES {
            return Err(TreeError::Budget);
        }
        let object = Object::from_bytes(raw);
        if let Some(old) = self.get(&object)? {
            if old != raw {
                return Err(TreeError::Integrity);
            }
            return Ok(object);
        }
        let nodes = self.nodes.checked_add(1).ok_or(TreeError::Budget)?;
        let bytes = self
            .bytes
            .checked_add(raw.len() as u64)
            .ok_or(TreeError::Budget)?;
        if nodes > self.max_nodes || bytes > self.max_bytes {
            return Err(TreeError::Budget);
        }
        if self.memory.put(raw)? != object {
            return Err(TreeError::Integrity);
        }
        self.nodes = nodes;
        self.bytes = bytes;
        Ok(object)
    }
}

fn unchanged(before: &Checkpoint, bounds: Limits, after: &MetadataReader) -> Result<()> {
    if after.checkpoint() != before || after.limits() != bounds {
        bail!("metadata changed during capture preview");
    }
    Ok(())
}

fn inspect(profile: &Path, output: &Path) -> Result<Value> {
    let raw = read_bytes(profile, 128 * 1024, None)?;
    let capture: Capture = serde_json::from_value(identity::parse_json(&raw)?)?;
    let contract = Contract::parse(&capture.schema, capture.request.clone())?;
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("preview output parent missing"))?;
    if !output.is_absolute() || output.file_name().is_none() || output.symlink_metadata().is_ok() {
        bail!("preview requires a new absolute output");
    }
    for root in [parent, &capture.metadata_root, &capture.metadata_cache] {
        existing_absolute(root)?;
    }
    if fs::canonicalize(parent)?.starts_with(fs::canonicalize(&capture.metadata_root)?) {
        bail!("preview report must remain outside metadata");
    }
    let simple = matches!(&contract, Contract::Browser(_) | Contract::Delivery(_));
    if simple == capture.preparation_cache.is_some()
        || !capture.cache.cache_roots.contains(&capture.metadata_cache)
        || capture
            .preparation_cache
            .as_ref()
            .is_some_and(|root| !capture.cache.cache_roots.contains(root))
    {
        bail!("preview requires the same explicit capture stores");
    }
    let coordination = capture
        .cache
        .coordination_root
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("preview coordination missing"))?;
    existing_absolute(coordination)?;
    let canonical_parent = fs::canonicalize(parent)?;
    for root in capture
        .cache
        .cache_roots
        .iter()
        .map(|root| root.join("objects/sha256"))
        .chain(std::iter::once(coordination.clone()))
    {
        if canonical_parent.starts_with(fs::canonicalize(root)?) {
            bail!("preview report must remain outside cache objects and coordination");
        }
    }
    if let Some(root) = &capture.preparation_cache {
        existing_absolute(root)?;
    }
    let volume = admission::volume_id(&capture.metadata_cache)
        .map_err(|_| anyhow::anyhow!("preview volume unavailable"))?;
    for root in [parent, capture.metadata_root.as_path()] {
        if admission::volume_id(root).map_err(|_| anyhow::anyhow!("preview volume unavailable"))?
            != volume
        {
            bail!("preview output must share the declared metadata volume");
        }
    }
    let wait = MetadataWait::default();
    let reader =
        wait.run(|| MetadataReader::open(&capture.metadata_root, &capture.metadata_minimum))?;
    let baseline = reader.checkpoint().clone();
    let bounds = reader.limits();
    // Only scene compilation needs files. Episode/browser/delivery freezes use
    // exactly the same algorithms without creating compilation scratch.
    let preparation = if matches!(&contract, Contract::Scene(_)) {
        Some(preparation::Preparation::open_on(
            &capture.cache,
            &capture.metadata_root,
            capture
                .preparation_cache
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("preview preparation missing"))?,
            1,
        )?)
    } else {
        None
    };
    let before = preparation.as_ref().map(|p| p.path().join("capture"));
    let mut source = crate::tig_inputs::Source::open(serde_json::from_value(capture.source)?)?;
    let limits = TreeLimits::default();
    let mut overlay = Overlay::new(
        &reader,
        limits.max_writes as u64,
        limits.max_write_bytes as u64,
    );
    let context = contract.freeze(&mut source, before.as_deref(), &mut overlay)?;
    let proposed_nodes = baseline
        .nodes()
        .checked_add(overlay.nodes)
        .ok_or_else(|| anyhow::anyhow!("preview count overflow"))?;
    let proposed_bytes = baseline
        .bytes()
        .checked_add(overlay.bytes)
        .ok_or_else(|| anyhow::anyhow!("preview size overflow"))?;
    let end = wait.run(|| MetadataReader::open(&capture.metadata_root, &baseline))?;
    unchanged(&baseline, bounds, &end)?;
    let report = json!({
        "schema":"reel.tig-native-capture-budget.experimental-1",
        "profile":Object::from_bytes(&raw),"source_checkpoint":source.checkpoint(),
        "context_kind":match contract { Contract::Scene(_)=>"scene",Contract::Episode(_)=>"episode",Contract::Delivery(_)=>"delivery",Contract::Browser(_)=>"browser" },
        "baseline":baseline,"limits":bounds,"new_nodes":overlay.nodes,"new_bytes":overlay.bytes,
        "proposed_nodes":proposed_nodes,"proposed_bytes":proposed_bytes,
        "node_limit_sufficient":proposed_nodes<=bounds.nodes,"logical_byte_limit_sufficient":proposed_bytes<=bounds.bytes,
        "transient_context":context.selection(),"transient_archive":context.archive(),
        "metadata_checkpoint_stable":true,"metadata_nodes_persisted":false,"context_stored":false,
        "database_capacity_verified":false,"physical_capacity_reserved":false,
        "complete_job_admission":false,"source_adopted":false,"media_rendered":false,
        "production_claim":null,"approval":false,"authorizes_eviction":false
    });
    let bytes = identity::canonical_json(&report)?;
    if bytes.len() > MAX_OUTPUT {
        bail!("preview report exceeds bound");
    }
    if let Some(preparation) = preparation {
        preparation.finish()?;
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("preview report publication refused"))?;
    Ok(
        json!({"schema":"reel.tig-native-capture-budget-summary.experimental-1",
        "new_nodes":overlay.nodes,"new_bytes":overlay.bytes,
        "node_limit_sufficient":proposed_nodes<=bounds.nodes,"logical_byte_limit_sufficient":proposed_bytes<=bounds.bytes,
        "metadata_nodes_persisted":false,"context_stored":false,"complete_job_admission":false,
        "source_adopted":false,"media_rendered":false,"approval":false,"authorizes_eviction":false}),
    )
}

/// Explicit diagnostic only. Never turns transient nodes into a capture ticket.
pub fn inspect_capture_budget(profile: &Path, output: &Path) -> Result<Value> {
    inspect(profile, output)
        .map_err(|error| capture_failure(error.context(CaptureStage("budget preview"))))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Immutable(MemoryNodes);
    impl NodeStore for Immutable {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            self.0.get(object)
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            panic!("preview must never write existing provider")
        }
    }
    #[test]
    fn overlay_verifies_existing_nodes_and_deduplicates_new_nodes_without_writes() {
        let mut memory = MemoryNodes::default();
        let old = memory.put(b"old").unwrap();
        let existing = Immutable(memory);
        let mut overlay = Overlay::new(&existing, 1, 3);
        assert_eq!(overlay.put(b"old").unwrap(), old);
        let new = overlay.put(b"new").unwrap();
        assert_eq!(overlay.put(b"new").unwrap(), new);
        assert_eq!((overlay.nodes, overlay.bytes), (1, 3));
        assert_eq!(overlay.get(&old).unwrap(), Some(b"old".to_vec()));
        assert_eq!(overlay.get(&new).unwrap(), Some(b"new".to_vec()));
        assert_eq!(overlay.put(b"extra"), Err(TreeError::Budget));
        assert_eq!((overlay.nodes, overlay.bytes), (1, 3));
        let wrong_size = Object {
            sha256: old.sha256,
            bytes: 1,
        };
        assert_eq!(overlay.get(&wrong_size), Err(TreeError::Integrity));
    }
    #[test]
    fn corrupt_existing_provider_and_oversized_nodes_refuse() {
        struct Corrupt;
        impl NodeStore for Corrupt {
            fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
                Ok(Some(b"wrong".to_vec()))
            }
            fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
                unreachable!()
            }
        }
        let mut overlay = Overlay::new(&Corrupt, 10, 100);
        assert_eq!(overlay.put(b"new"), Err(TreeError::Integrity));
        assert_eq!((overlay.nodes, overlay.bytes), (0, 0));
        assert_eq!(
            overlay.put(&vec![0; MAX_NODE_BYTES + 1]),
            Err(TreeError::Budget)
        );
    }
    #[test]
    fn full_real_store_can_be_previewed_without_persisting_and_changed_checkpoint_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("metadata");
        let bounds = Limits {
            nodes: 2,
            bytes: 4096,
            database_bytes: 1024 * 1024,
            min_free_bytes: 65536,
        };
        let mut store = MetadataStore::create(&root, bounds).unwrap();
        store.put(b"old").unwrap();
        let first = store.finish().unwrap();
        drop(store);
        let reader = MetadataReader::open(&root, &first).unwrap();
        {
            let mut overlay = Overlay::new(&reader, 10, 1024);
            overlay.put(b"old").unwrap();
            overlay.put(b"new-one").unwrap();
            overlay.put(b"new-two").unwrap();
            assert!(first.nodes() + overlay.nodes > bounds.nodes);
        }
        let same = MetadataReader::open(&root, &first).unwrap();
        assert_eq!(same.checkpoint(), &first);
        unchanged(&first, bounds, &same).unwrap();
        drop(same);
        let mut writer = MetadataStore::open(&root, &first).unwrap();
        writer.put(b"another-writer").unwrap();
        let second = writer.finish().unwrap();
        drop(writer);
        let changed = MetadataReader::open(&root, &second).unwrap();
        assert!(unchanged(&first, bounds, &changed).is_err());
    }
}
