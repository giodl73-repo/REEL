//! Exact native delivery receipts are output manifests, not producer authority.
//! Existing cache reads only: no hydration, copies, setup or publication.
use crate::scene_delivery::{FileRef, InputFiles, Receipt};
use anyhow::{Result, anyhow, bail};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
use tig_core::{Digest, LogicalPath, Object};
use tig_store::existing::{ByteSource, Profile, ReadCache, RequiredProtection, VerifiedRead};

const RECEIPT_MAX: u64 = 4 * 1024 * 1024;
const OUTPUTS_MAX: usize = 4096;
const RECEIPT_NAME: &str = "receipt.json";

/// Owns every verified output guard, deduplicated by hash. Keep this owner alive
/// for the entire synchronous check/consume operation, including subprocesses.
/// Caller supplies an admitted receipt pin; opening does not prove current-source
/// correspondence, delivery selection, retained custody or eviction eligibility.
pub struct Bundle {
    receipt: Receipt,
    receipt_ref: FileRef,
    names: BTreeMap<String, Object>,
    held: BTreeMap<Digest, VerifiedRead>,
}
impl Bundle {
    pub fn open(profile: Profile, receipt: &Object) -> Result<Self> {
        if !cfg!(windows) || profile.required != RequiredProtection::PreventMutation {
            bail!("mandatory output path protection unavailable");
        }
        if receipt.bytes > RECEIPT_MAX {
            bail!("delivery receipt exceeds metadata budget");
        }
        let limits = profile.limits;
        let mut cache = ReadCache::new(profile)?;
        let mut guard = cache.open_verified(receipt)?;
        let mut bytes = Vec::new();
        (&mut guard).take(RECEIPT_MAX + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != receipt.bytes {
            bail!("delivery receipt size changed");
        }
        let parsed: Receipt = serde_json::from_value(tig_core::identity::parse_json(&bytes)?)?;
        if parsed.schema != "reel.scene-delivery-receipt.v0.1"
            || parsed.outputs.is_empty()
            || parsed.outputs.len() > OUTPUTS_MAX
        {
            bail!("unsupported or unbounded delivery receipt");
        }
        let mut names = BTreeMap::from([(RECEIPT_NAME.to_owned(), receipt.clone())]);
        let mut objects = BTreeMap::from([(receipt.sha256.clone(), receipt.bytes)]);
        for (name, file) in &parsed.outputs {
            LogicalPath::parse(name)?;
            if file.path.to_str() != Some(name.as_str()) || name == RECEIPT_NAME {
                bail!("delivery output name mismatch");
            }
            let object = Object {
                sha256: Digest::parse(&file.sha256)?,
                bytes: file.bytes,
            };
            if let Some(previous) = objects.insert(object.sha256.clone(), object.bytes)
                && previous != object.bytes
            {
                bail!("conflicting delivery object sizes");
            }
            names.insert(name.clone(), object);
        }
        let total = objects
            .values()
            .try_fold(0_u64, |sum, size| sum.checked_add(*size))
            .ok_or_else(|| anyhow!("delivery byte budget overflow"))?;
        if objects.len() > limits.live_readers as usize
            || objects.len() > limits.requests as usize
            || total > limits.total_bytes
            || objects.values().any(|size| *size > limits.object_bytes)
        {
            bail!("delivery outputs exceed reader budget");
        }
        let mut held = BTreeMap::from([(receipt.sha256.clone(), guard)]);
        for (hash, size) in objects {
            if let std::collections::btree_map::Entry::Vacant(entry) = held.entry(hash.clone()) {
                let verified = cache.open_verified(&Object {
                    sha256: hash.clone(),
                    bytes: size,
                })?;
                entry.insert(verified);
            }
        }
        Ok(Self {
            receipt: parsed,
            receipt_ref: FileRef {
                path: RECEIPT_NAME.into(),
                sha256: receipt.sha256.as_str().into(),
                bytes: receipt.bytes,
            },
            names,
            held,
        })
    }
    pub fn receipt_ref(&self) -> &FileRef {
        &self.receipt_ref
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
    /// Exact retained closure for a caller's version/custody adapter.
    pub fn objects(&self) -> impl Iterator<Item = &Object> {
        self.held.values().map(VerifiedRead::object)
    }
}
impl InputFiles for Bundle {
    fn verified_path(&self, expected: &FileRef) -> Result<PathBuf> {
        let name = expected
            .path
            .to_str()
            .ok_or_else(|| anyhow!("non-Unicode output name"))?;
        LogicalPath::parse(name)?;
        let object = self
            .names
            .get(name)
            .ok_or_else(|| anyhow!("unbound delivery output"))?;
        if object.sha256.as_str() != expected.sha256 || object.bytes != expected.bytes {
            bail!("delivery output pin changed");
        }
        self.held
            .get(&object.sha256)
            .ok_or_else(|| anyhow!("missing output guard"))?
            .with_path(Path::to_path_buf)
            .map_err(Into::into)
    }
}
