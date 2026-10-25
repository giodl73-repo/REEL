//! Explicit placement authority for compiled outputs, separate from authoring heads.
use crate::{Result, config, tiering, versions};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub bytes: u64,
    pub selected: bool,
    pub archive_authorized: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Map {
    schema: String,
    project_id: String,
    consumer_selections: BTreeMap<String, String>,
    pub outputs: BTreeMap<String, Entry>,
}

pub(crate) fn load(root: &Path, policy: &tiering::Policy) -> Result<Option<Map>> {
    let Some(a) = &policy.output_retention else {
        return Ok(None);
    };
    if !crate::valid_oid(&a.sha256) || a.bytes == 0 || a.bytes > 8 * 1024 * 1024 {
        return Err("Invalid output retention control descriptor".into());
    }
    let p = crate::active_existing(&config(root)?, &a.sha256, a.bytes)?
        .ok_or("Output retention control absent from active caches; archive not probed")?;
    let map: Map = serde_json::from_slice(&tiering::read_bounded(&p, 8 * 1024 * 1024)?)?;
    if map.schema != "reel.output-retention-map.v1"
        || map.project_id != policy.project_id
        || map.consumer_selections.is_empty()
        || map.consumer_selections.len() > 64
        || map
            .consumer_selections
            .values()
            .any(|h| !crate::valid_oid(h))
        || map.outputs.is_empty()
        || map.outputs.len() > policy.max_objects
        || map.outputs.keys().any(|h| !crate::valid_oid(h))
    {
        return Err("Invalid output retention map/project binding".into());
    }
    Ok(Some(map))
}

impl Map {
    pub fn validate_selection(&self, root: &Path, state: &versions::State) -> Result<()> {
        let key = root.canonicalize()?.to_string_lossy().into_owned();
        let actual = crate::demotion::output_selection_stamp(root, state)?;
        if self.consumer_selections.get(&key) != Some(&actual) {
            return Err(
                "Output retention selection changed or consumer is undeclared; refresh the map"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn validate_primary_inputs(&self, state: &versions::State) -> Result<()> {
        for version in state.versions.values() {
            if let Some(c) = &version.dependency_contract
                && c.primary_inputs
                    .iter()
                    .any(|h| self.outputs.contains_key(h))
            {
                return Err(
                    "Output retention classification conflicts with a primary input".into(),
                );
            }
        }
        Ok(())
    }
    pub fn is_archival(&self, hash: &str) -> bool {
        self.outputs
            .get(hash)
            .is_some_and(|e| !e.selected && e.archive_authorized)
    }
    pub fn filter_authoring_heads(&self, hashes: &mut BTreeSet<String>) {
        // Unknowns and unproven candidates remain held. Only explicitly classified,
        // explicitly archived outputs can lose an authoring-head hold.
        hashes.retain(|h| !self.is_archival(h));
    }
    pub fn apply(
        &self,
        root: &Path,
        state: &versions::State,
        registered: &BTreeMap<String, u64>,
        current: &mut BTreeSet<String>,
        selected: &mut BTreeSet<String>,
    ) -> Result<()> {
        self.validate_selection(root, state)?;
        self.validate_primary_inputs(state)?;
        for (h, e) in &self.outputs {
            if registered.get(h).is_some_and(|n| *n != e.bytes) {
                return Err("Output retention byte count differs from registered asset".into());
            }
        }
        self.filter_authoring_heads(current);
        self.filter_authoring_heads(selected);
        selected.extend(
            self.outputs
                .iter()
                .filter(|(_, e)| e.selected)
                .map(|(h, _)| h.clone()),
        );
        Ok(())
    }
}
