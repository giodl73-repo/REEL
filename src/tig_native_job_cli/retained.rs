//! Readback of this coordinator's private completed phase, never arbitrary JSON.
use super::*;
use tig_core::tree::{NodeStore, Tree};
pub(super) struct Verified {
    pub reader: MetadataReader,
    outputs: BTreeMap<String, Object>,
}
impl Verified {
    pub fn object(&self, name: &str) -> Result<&Object> {
        self.outputs
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("retained output missing"))
    }
    pub fn bytes(&self, result: &PhaseResult, name: &str, max: usize) -> Result<Vec<u8>> {
        let path = result.value["outputs"][name]["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("retained output locator missing"))?;
        read_bytes(Path::new(path), max, Some(self.object(name)?))
    }
}
pub(super) fn owned_selection(
    journal: &mut SourceJournal,
    source: &Key,
    owner: &Key,
    revision: u64,
) -> Result<Selection> {
    let state = journal
        .source(source)?
        .ok_or_else(|| anyhow::anyhow!("completed source missing"))?;
    if state.revision != revision || &state.owner != owner || state.pending.is_some() {
        bail!("completed source owner/revision differs");
    }
    let current = state
        .current
        .ok_or_else(|| anyhow::anyhow!("completed result root missing"))?;
    if current.revision != revision {
        bail!("result is not the latest completed transition");
    }
    Ok(current.selection)
}
pub(super) fn verify(
    profile: &Profile,
    result: &PhaseResult,
    journal: &mut SourceJournal,
) -> Result<Verified> {
    let selection = owned_selection(
        journal,
        &Key::parse(&profile.source_id)?,
        &Key::parse(&profile.owner)?,
        result.revision,
    )?;
    let mut reader = MetadataReader::open(&profile.worker.metadata_root, &result.minimum)?;
    let entries = Tree::from_root(selection.root)
        .entries(&mut reader, TreeLimits::default())?
        .0;
    let get = |name: &str| -> Result<&Object> {
        entries
            .get(&LogicalPath::parse(name)?)
            .ok_or_else(|| anyhow::anyhow!("retained phase reference missing"))
    };
    let header = get("retention/identity")?;
    let outputs = result.value["outputs"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("completed phase outputs missing"))?;
    if header.sha256 != selection.context
        || get("source/context")? != &result.resources.root
        || entries.len() != outputs.len() + 4
    {
        bail!("retained phase root differs");
    }
    let mut pins = BTreeMap::new();
    for (name, value) in outputs {
        let object = Object {
            sha256: Digest::parse(
                value["sha256"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("retained output hash missing"))?,
            )?,
            bytes: value["bytes"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("retained output size missing"))?,
        };
        if get(&format!(
            "outputs/{}",
            Digest::blob(name.as_bytes()).as_str()
        ))? != &object
        {
            bail!("retained phase output pin differs");
        }
        pins.insert(name.clone(), object);
    }
    let header = identity::parse_json(
        &reader
            .get(header)?
            .ok_or_else(|| anyhow::anyhow!("retained phase header missing"))?,
    )?;
    if header["schema"] != "reel.tig-runner-retention.experimental-1"
        || header["succeeded"] != true
        || header["source"] != serde_json::to_value(&result.resources)?
        || header["journal"] != serde_json::to_value(journal.identity())?
        || header["owner"] != profile.owner
        || header["source_id"] != profile.source_id
        || header["revision"]
            != result
                .revision
                .checked_sub(2)
                .ok_or_else(|| anyhow::anyhow!("invalid completed phase revision"))?
        || header["outputs"] != outputs.len()
        || header["job_id"] != result.value["job_id"]
    {
        bail!("retained phase identity differs");
    }
    Ok(Verified {
        reader,
        outputs: pins,
    })
}
