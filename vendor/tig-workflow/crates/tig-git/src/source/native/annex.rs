//! Optional typed metadata bound to explicit annex leaves in one captured view.
//! This proves authored identities only, never hydration, protection or custody.
use super::{AssetSnapshot, Error, Limits, Repository, Result, Scope, Snapshot};
use std::collections::BTreeMap;
use tig_assets::resolve::AssetResolver;
use tig_core::LogicalPath;

/// Explicit relationship supplied by the catalogue adapter, not basename magic.
/// Existing records still declare the expected selected/stream byte identities.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub selected: LogicalPath,
    pub streams: BTreeMap<String, LogicalPath>,
}

pub struct Catalogue {
    pub scope: Scope,
    pub bindings: BTreeMap<String, Binding>,
}

/// Both views derive from the same captured Git listing. Metadata checkpoint and
/// payload selection digest remain separate; neither grants effect permission.
pub struct BoundAssets {
    pub assets: AssetSnapshot,
    pub payloads: crate::source::Extraction,
}

impl Repository {
    pub fn native_annex(
        &self,
        snapshot: &Snapshot,
        records: Scope,
        definitions: Scope,
        catalogue: &Catalogue,
        limits: Limits,
    ) -> Result<BoundAssets> {
        // Bound the declaration before reading any metadata or Git blobs.
        limits.validate()?;
        let bindings = &catalogue.bindings;
        let declaration_bytes = bindings.iter().try_fold(0usize, |n, (id, binding)| {
            let n = n
                .checked_add(id.len())?
                .checked_add(binding.selected.as_str().len())?;
            binding.streams.iter().try_fold(n, |n, (slot, path)| {
                n.checked_add(slot.len())?.checked_add(path.as_str().len())
            })
        });
        if bindings.len() > limits.documents
            || declaration_bytes.is_none_or(|n| n > limits.bytes)
            || bindings
                .values()
                .try_fold(0usize, |n, b| {
                    n.checked_add(b.streams.len())?.checked_add(1)
                })
                .is_none_or(|n| n > limits.documents)
        {
            return Err(Error::Budget);
        }
        let assets = self.native(snapshot, records, definitions, limits)?;
        let payloads = self.extract_annex(snapshot, catalogue.scope.clone())?;
        if !payloads.complete() {
            return Err(Error::Incomplete);
        }
        if bindings.len() != assets.len() {
            return Err(Error::Incomplete);
        }
        let paths: BTreeMap<_, _> = payloads
            .bindings()
            .iter()
            .map(|b| (&b.path, &b.object))
            .collect();
        for (id, binding) in bindings {
            let asset = assets.resolve_id(id).map_err(|_| Error::Incomplete)?;
            let selected = asset.selected().map_err(|_| Error::Incomplete)?;
            if paths.get(&binding.selected).copied() != Some(selected) {
                return Err(Error::Integrity);
            }
            if binding.streams.len() != asset.stream_bindings().len() {
                return Err(Error::Incomplete);
            }
            for stream in asset.stream_bindings() {
                let path = binding.streams.get(&stream.slot).ok_or(Error::Incomplete)?;
                if paths.get(path).copied() != Some(&stream.object) {
                    return Err(Error::Integrity);
                }
            }
        }
        Ok(BoundAssets { assets, payloads })
    }
}
