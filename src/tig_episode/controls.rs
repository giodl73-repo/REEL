//! Exact bounded parent metadata reads; no media hydration or local fallback.
use crate::{
    episode_conform::{Manifest, SCHEMA},
    scene_delivery::{FileRef, InputFiles},
    tig_scene_outputs::output_name,
};
use anyhow::{Result, bail};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};
use tig_core::tree::{NodeStore, Tree, TreeLimits};
use tig_core::{Digest, Object, identity};
use tig_store::existing::{ByteSource, Profile, ReadCache, RequiredProtection, VerifiedRead};

const MAX: u64 = 4 * 1024 * 1024;
/// Existing Tig tree format for caller-admitted derived native metadata names.
/// Tree nodes stay in the metadata provider; named bodies use existing caches.
/// No second authored inventory, source authority or production approval implied.
pub struct ControlTree {
    names: BTreeMap<String, Object>,
    held: BTreeMap<Digest, VerifiedRead>,
}
impl ControlTree {
    pub fn open(profile: Profile, root: &Object, store: &mut impl NodeStore) -> Result<Self> {
        if !cfg!(windows) || profile.required != RequiredProtection::PreventMutation {
            bail!("mandatory scene control path protection unavailable");
        }
        let entries = Tree::from_root(root.clone())
            .entries(
                store,
                TreeLimits {
                    max_entries: 64,
                    max_read_bytes: 1024 * 1024,
                    ..TreeLimits::default()
                },
            )?
            .0;
        if entries.is_empty() {
            bail!("empty scene control tree");
        }
        let mut names = BTreeMap::new();
        let mut objects = BTreeMap::new();
        for (name, object) in entries {
            let name = output_name(std::path::Path::new(name.as_str()))?;
            if object.bytes > MAX
                || names.insert(name, object.clone()).is_some()
                || objects
                    .insert(object.sha256.clone(), object.bytes)
                    .is_some_and(|old| old != object.bytes)
            {
                bail!("scene control tree name, length or bound differs");
            }
        }
        let bytes = objects
            .values()
            .try_fold(0u64, |sum, n| sum.checked_add(*n))
            .ok_or_else(|| anyhow::anyhow!("scene control byte overflow"))?;
        let limits = profile.limits;
        if bytes > 16 * 1024 * 1024
            || objects.len() > limits.requests as usize
            || objects.len() > limits.live_readers as usize
            || bytes > limits.total_bytes
            || objects.values().any(|n| *n > limits.object_bytes)
        {
            bail!("scene control tree exceeds reader budget");
        }
        let mut cache = ReadCache::new(profile)?;
        let mut held = BTreeMap::new();
        for (sha256, bytes) in objects {
            held.insert(
                sha256.clone(),
                cache.open_verified(&Object { sha256, bytes })?,
            );
        }
        Ok(Self { names, held })
    }
    pub fn objects(&self) -> impl Iterator<Item = &Object> {
        self.held.values().map(VerifiedRead::object)
    }
}
impl InputFiles for ControlTree {
    fn verified_path(&self, reference: &FileRef) -> Result<PathBuf> {
        let name = output_name(&reference.path)?;
        let object = self
            .names
            .get(&name)
            .ok_or_else(|| anyhow::anyhow!("scene control name is not admitted"))?;
        if object.sha256.as_str() != reference.sha256 || object.bytes != reference.bytes {
            bail!("scene control reference differs from admitted pin");
        }
        self.held
            .get(&object.sha256)
            .ok_or_else(|| anyhow::anyhow!("scene control guard missing"))?
            .with_path(|path| path.to_path_buf())
            .map_err(Into::into)
    }
}
/// Keeps the admitted conform and each top-level control under effective path/
/// byte protection. Does not grant source currency, complete scene-control
/// closure, delivery selection, custody or eviction permission.
pub struct ParentControls {
    manifest: Manifest,
    reference: FileRef,
    names: BTreeMap<String, Object>,
    held: BTreeMap<Digest, VerifiedRead>,
}
impl ParentControls {
    pub fn open(profile: Profile, conform: &Object) -> Result<Self> {
        if !cfg!(windows) || profile.required != RequiredProtection::PreventMutation {
            bail!("mandatory episode control path protection unavailable");
        }
        if conform.bytes > MAX {
            bail!("episode conform exceeds metadata budget");
        }
        let limits = profile.limits;
        let mut cache = ReadCache::new(profile)?;
        let mut guard = cache.open_verified(conform)?;
        guard.with_path(|_| ())?;
        guard.seek(SeekFrom::Start(0))?;
        let mut raw = Vec::new();
        (&mut guard).take(MAX + 1).read_to_end(&mut raw)?;
        if raw.len() as u64 != conform.bytes {
            bail!("episode conform size differs");
        }
        let manifest: Manifest = serde_json::from_value(identity::parse_json(&raw)?)?;
        if manifest.schema != SCHEMA
            || manifest.episode_id.is_empty()
            || !matches!(manifest.language.as_str(), "en" | "es")
            || manifest.segments.is_empty()
            || manifest.segments.len() > 128
        {
            bail!("invalid bounded parent control scope");
        }
        let mut names = BTreeMap::new();
        let mut objects = BTreeMap::from([(conform.sha256.clone(), conform.bytes)]);
        for item in [
            &manifest.catalog,
            &manifest.master_template_definition,
            &manifest.episode,
            &manifest.season_bindings,
            &manifest.episode_bindings,
        ]
        .into_iter()
        .chain(manifest.source_master_template.iter())
        {
            let name = output_name(&item.path)?;
            let object = Object {
                sha256: Digest::parse(&item.sha256)?,
                bytes: item.bytes,
            };
            if name == "manifest.json"
                || object.bytes > MAX
                || names
                    .insert(name, object.clone())
                    .is_some_and(|old| old != object)
                || objects
                    .insert(object.sha256.clone(), object.bytes)
                    .is_some_and(|old| old != object.bytes)
            {
                bail!("parent control alias, pin or metadata bound differs");
            }
        }
        let bytes = objects
            .values()
            .try_fold(0u64, |sum, n| sum.checked_add(*n))
            .ok_or_else(|| anyhow::anyhow!("parent control byte overflow"))?;
        if objects.len() > limits.requests as usize
            || objects.len() > limits.live_readers as usize
            || bytes > limits.total_bytes
            || objects.values().any(|n| *n > limits.object_bytes)
        {
            bail!("parent controls exceed reader budget");
        }
        let mut held = BTreeMap::from([(conform.sha256.clone(), guard)]);
        for (sha256, bytes) in objects {
            if let std::collections::btree_map::Entry::Vacant(slot) = held.entry(sha256.clone()) {
                slot.insert(cache.open_verified(&Object { sha256, bytes })?);
            }
        }
        names.insert("manifest.json".into(), conform.clone());
        Ok(Self {
            manifest,
            reference: FileRef {
                path: "manifest.json".into(),
                sha256: conform.sha256.as_str().into(),
                bytes: conform.bytes,
            },
            names,
            held,
        })
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn manifest_ref(&self) -> &FileRef {
        &self.reference
    }
    pub fn objects(&self) -> impl Iterator<Item = &Object> {
        self.held.values().map(VerifiedRead::object)
    }
}
impl InputFiles for ParentControls {
    fn verified_path(&self, reference: &FileRef) -> Result<PathBuf> {
        let name = output_name(&reference.path)?;
        let object = self
            .names
            .get(&name)
            .ok_or_else(|| anyhow::anyhow!("episode control name is not admitted"))?;
        if object.sha256.as_str() != reference.sha256 || object.bytes != reference.bytes {
            bail!("episode control reference differs from admitted pin");
        }
        self.held
            .get(&object.sha256)
            .ok_or_else(|| anyhow::anyhow!("episode control guard missing"))?
            .with_path(|path| path.to_path_buf())
            .map_err(Into::into)
    }
}
