//! Bounded exact-byte snapshots of native controls; no media materialization.
use crate::scene_delivery::{self, FileRef};
use anyhow::{Context, Result, bail};
use std::{collections::BTreeMap, fs, io::Read, path::Path};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{MemoryNodes, NodeStore, Tree, TreeLimits},
};
// Retain just the four native render controls, not the compiler's diagnostic
// inventories and intermediate authoring projections. Each remains exact bytes.
pub(crate) fn capture(
    preparation: &Path,
    relative: &Path,
    prefix: &str,
    metadata: &mut BTreeMap<Object, Vec<u8>>,
    total: &mut u64,
) -> Result<FileRef> {
    let relative = scene_delivery::control_name(relative)?;
    let path = preparation.join(&relative);
    let root = preparation.canonicalize()?;
    if !fs::symlink_metadata(&path)?.file_type().is_file()
        || path.canonicalize()? != root.join(&relative)
    {
        bail!("scene control is not a regular contained file");
    }
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut raw)?;
    *total = total
        .checked_add(raw.len() as u64)
        .context("scene control budget overflow")?;
    if raw.len() > 4 * 1024 * 1024 || *total > 16 * 1024 * 1024 {
        bail!("scene controls exceed metadata budget");
    }
    let object = Object::from_bytes(&raw);
    let name = relative
        .to_str()
        .context("non-Unicode scene control")?
        .replace('\\', "/");
    let file = FileRef {
        path: format!("{prefix}{name}").into(),
        sha256: object.sha256.as_str().into(),
        bytes: object.bytes,
    };
    metadata.insert(object, raw);
    Ok(file)
}
pub(crate) fn body<'a>(
    metadata: &'a BTreeMap<Object, Vec<u8>>,
    reference: &FileRef,
) -> Result<&'a [u8]> {
    let object = Object {
        sha256: Digest::parse(&reference.sha256)?,
        bytes: reference.bytes,
    };
    Ok(metadata
        .get(&object)
        .context("compiled scene control body missing")?)
}

// Existing Tig trees and bounded opaque metadata nodes frame large control
// bodies. These are generated provider data, never another authored inventory.
pub(crate) fn frame(
    bodies: &BTreeMap<Object, Vec<u8>>,
) -> Result<(Object, BTreeMap<Object, Vec<u8>>)> {
    let mut nodes = MemoryNodes::default();
    let mut names = BTreeMap::new();
    for (object, raw) in bodies {
        let prefix = object.sha256.as_str();
        let identity = identity::canonical_json(&serde_json::json!(object))?;
        names.insert(
            LogicalPath::parse(&format!("{prefix}/identity"))?,
            nodes.put(&identity)?,
        );
        for (index, part) in raw.chunks(tig_store::metadata::MAX_NODE_BYTES).enumerate() {
            names.insert(
                LogicalPath::parse(&format!("{prefix}/part-{index:04}"))?,
                nodes.put(part)?,
            );
        }
    }
    let (tree, _) = Tree::build(&names, &mut nodes, TreeLimits::default())?;
    let root = tree.root().clone();
    let storage = nodes
        .objects()
        .map(|(_, raw)| (Object::from_bytes(raw), raw.to_vec()))
        .collect();
    Ok((root, storage))
}
