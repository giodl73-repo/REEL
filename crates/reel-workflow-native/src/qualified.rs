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
        reel_assets::admission::lock(&path).map_err(|e| match e
            .downcast::<reel_assets::admission::MetadataBusy>()
        {
            Ok(busy) => anyhow::Error::new(*busy),
            Err(other) => match other.downcast::<std::io::Error>() {
                Ok(io) => anyhow::Error::new(*io),
                Err(other) => anyhow::anyhow!("{other}"),
            },
        })
    }
}
// Preserve the full qualified module, including its close/recovery code and
// original tests. The closing adapter calls this protocol without changing it.
include!(concat!(env!("OUT_DIR"), "/qualified_module.rs"));

#[cfg(test)]
mod fence_tests {
    use super::*;
    #[test]
    fn actor_contention_is_busy_without_parsing_a_message_or_releasing_another_fence() {
        let root = tempfile::tempdir().unwrap();
        let index = Digest::blob(b"synthetic index");
        let actor = Key::parse("retained").unwrap();
        let first = prepublication::actor_fence(&index, root.path(), &actor).unwrap();
        let err = prepublication::actor_fence(&index, root.path(), &actor)
            .err()
            .unwrap();
        assert_eq!(
            crate::admission::native_error(err).kind,
            reel_workflow::ErrorKind::Busy
        );
        assert!(prepublication::actor_fence(&index, root.path(), &actor).is_err());
        drop(first);
        assert!(prepublication::actor_fence(&index, root.path(), &actor).is_ok());
    }
}
