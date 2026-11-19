//! Type/fence shim for unchanged EVID-151 REEL module source. No source engine
//! or new slot protocol. The public workflow adapter admits the resource closure.
use anyhow::{Result, bail};
use std::{collections::BTreeMap, path::Path};
#[cfg(test)]
use tig_core::tree::TreeLimits;
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree},
};
#[cfg(test)]
use tig_store::source_claims::SourceJournal;
use tig_store::{
    Key,
    metadata::{MetadataReader, MetadataStore},
    source_claims::Selection,
};

pub(crate) mod prepublication {
    use super::*;
    pub type Scope = crate::admission::Scope;
    // Same domain and zero-wait metadata fence as the qualified REEL integration.
    pub(crate) fn actor_fence(
        index: &Digest,
        root: &Path,
        actor: &Key,
    ) -> Result<reel_assets::admission::MetadataLock> {
        if !root.is_absolute() || !root.is_dir() {
            bail!("existing actor coordination required");
        }
        let path = root
            .join("tig-native-publication")
            .join(index.as_str())
            .join(Digest::blob(actor.as_str().as_bytes()).as_str());
        reel_assets::admission::lock(&path).map_err(|e| anyhow::anyhow!("{e}"))
    }
}
// Preserve the full qualified module, including its close/recovery code and
// original tests. The closing adapter calls this protocol without changing it.
#[allow(dead_code)]
#[rustfmt::skip]
#[path = "../../../vendor/reel-workflow-lifecycle/src/tig_job_runner/slots.rs"]
pub(crate) mod slots;
