//! Exact, bounded projection of compiled episode controls already in metadata.
//! This authenticates admitted pins, not producer success or creative authority.
use super::*;
use std::cell::Cell;
use tig_core::tree::{NodeStore, Tree, TreeError};

const SCHEMA: &str = "reel.tig-episode-control-inspection-profile.experimental-1";
const MAX_CONTROL_BYTES: u64 = 4 * 1024 * 1024;
const MAX_NODE_READS: u64 = 2048;
const MAX_NODE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    resources: Selection,
    recipe: Digest,
    manifest_sha256: Digest,
    max_control_bytes: u64,
}

// Control bodies are larger than Tig's small identity documents. Interpret only
// this tag; the exact remaining bytes belong to the existing conform consumer.
// Derived struct deserialization also refuses a duplicate schema tag.
#[derive(Deserialize)]
struct ConformKind {
    schema: String,
}

struct BoundedReader<'a, T> {
    inner: &'a mut T,
    reads: Cell<u64>,
    bytes: Cell<u64>,
}
impl<T: NodeStore> NodeStore for BoundedReader<'_, T> {
    fn get(&self, object: &Object) -> std::result::Result<Option<Vec<u8>>, TreeError> {
        let reads = self.reads.get().checked_add(1).ok_or(TreeError::Budget)?;
        let bytes = self
            .bytes
            .get()
            .checked_add(object.bytes)
            .ok_or(TreeError::Budget)?;
        if reads > MAX_NODE_READS || bytes > MAX_NODE_BYTES {
            return Err(TreeError::Budget);
        }
        self.reads.set(reads);
        self.bytes.set(bytes);
        self.inner.get(object)
    }
    fn put(&mut self, _: &[u8]) -> std::result::Result<Object, TreeError> {
        Err(TreeError::Store("read-only control inspection".into()))
    }
}

fn lookup(tree: &Tree, name: &str, reader: &mut impl NodeStore) -> Result<Object> {
    tree.get(&LogicalPath::parse(name)?, reader, TreeLimits::default())?
        .0
        .ok_or_else(|| anyhow::anyhow!("named metadata link absent"))
}
fn node(reader: &mut impl NodeStore, pin: &Object) -> Result<Vec<u8>> {
    let raw = reader
        .get(pin)?
        .ok_or_else(|| anyhow::anyhow!("metadata absent"))?;
    if Object::from_bytes(&raw) != *pin {
        bail!("metadata bytes differ");
    }
    Ok(raw)
}

fn controls(
    resources: &Selection,
    recipe: &Digest,
    manifest: &Digest,
    maximum: u64,
    store: &mut impl NodeStore,
) -> Result<Value> {
    if !(1..=MAX_CONTROL_BYTES).contains(&maximum) {
        bail!("invalid control allowance");
    }
    let mut reader = BoundedReader {
        inner: store,
        reads: Cell::new(0),
        bytes: Cell::new(0),
    };
    let job = Tree::from_root(resources.root.clone());
    let header = lookup(&job, "job/identity", &mut reader)?;
    if header.sha256 != resources.context {
        bail!("job identity differs");
    }
    let header = identity::parse_json(&node(&mut reader, &header)?)?;
    if !matches!(
        header["schema"].as_str(),
        Some(
            "tig.native-job-context.experimental-1"
                | "tig.native-job-context.experimental-2"
                | "tig.native-job-context.experimental-3"
        )
    ) || header["recipe"].as_str() != Some(recipe.as_str())
    {
        bail!("admitted job header differs");
    }
    let root = lookup(&job, "input-evidence/current-controls-root", &mut reader)?;
    let frame = Tree::from_root(root.clone());
    let prefix = manifest.as_str();
    let identity_pin = lookup(&frame, &format!("{prefix}/identity"), &mut reader)?;
    let value = identity::parse_json(&node(&mut reader, &identity_pin)?)?;
    if value
        .as_object()
        .is_none_or(|v| v.len() != 2 || !v.contains_key("sha256") || !v.contains_key("bytes"))
    {
        bail!("invalid control identity");
    }
    let object: Object = serde_json::from_value(value)?;
    if &object.sha256 != manifest || object.bytes == 0 || object.bytes > maximum {
        bail!("control identity differs or exceeds allowance");
    }
    let size = usize::try_from(object.bytes)?;
    let chunk = tig_store::metadata::MAX_NODE_BYTES;
    let mut raw = Vec::with_capacity(size);
    for index in 0..size.div_ceil(chunk) {
        let pin = lookup(&frame, &format!("{prefix}/part-{index:04}"), &mut reader)?;
        let remaining = size - raw.len();
        if pin.bytes != remaining.min(chunk) as u64 {
            bail!("control chunk length differs");
        }
        raw.extend(node(&mut reader, &pin)?);
    }
    if Object::from_bytes(&raw) != object {
        bail!("compiled control hash differs");
    }
    let value: ConformKind = serde_json::from_slice(&raw)?;
    if value.schema != "reel.episode-conform.v1" {
        bail!("not an episode conform control");
    }
    Ok(json!({
        "schema":"reel.tig-episode-control-inspection.experimental-1",
        "resources":resources,"recipe_admission":recipe,"control_frame":root,
        "manifest":{"object":object,"utf8":String::from_utf8(raw)?},
        "node_reads":reader.reads.get(),"node_bytes_requested":reader.bytes.get(),
        "job_identity_and_recipe_pin_verified":true,"control_bytes_verified":true,
        "full_context_replay":false,"materialized_media_bytes":0,"programs_started":0,
        "metadata_writes":0,"execution_attested":false,"production_claim":null,
        "approval_transferred":false,"authorizes_production":false,"authorizes_eviction":false
    }))
}

/// Private operational stdout only. Existing metadata is opened read-only;
/// inspection never adopts, renders, publishes, initializes or writes files.
pub fn inspect_episode_controls(path: &Path) -> Result<Value> {
    let mut stage = "profile admission";
    (|| {
        let p: Profile = read(path, 128 * 1024, None)?;
        if p.schema != SCHEMA
            || !p.metadata_root.is_absolute()
            || !(1..=MAX_CONTROL_BYTES).contains(&p.max_control_bytes)
        {
            bail!("invalid profile");
        }
        stage = "existing metadata opening";
        let mut reader = MetadataReader::open(&p.metadata_root, &p.metadata_minimum)?;
        stage = "named compiled control readback";
        let mut report = controls(
            &p.resources,
            &p.recipe,
            &p.manifest_sha256,
            p.max_control_bytes,
            &mut reader,
        )?;
        report["metadata_observed"] = serde_json::to_value(reader.checkpoint())?;
        Ok(report)
    })()
    .map_err(|_: anyhow::Error| anyhow::anyhow!("episode control inspection refused at {stage}"))
}

#[cfg(test)]
mod tests;
