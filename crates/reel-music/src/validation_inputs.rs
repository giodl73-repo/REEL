//! Operation-local validation snapshots. No discovery, storage or disk fallback.
use anyhow::{Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

type ReadBytes<'a> = dyn FnMut(&Path) -> Result<Vec<u8>> + 'a;
pub struct ValidationInputs<'a> {
    read: Option<&'a mut ReadBytes<'a>>,
    snapshots: BTreeMap<PathBuf, Vec<u8>>,
    total: usize,
}
impl<'a> ValidationInputs<'a> {
    pub fn filesystem() -> Self {
        Self {
            read: None,
            snapshots: BTreeMap::new(),
            total: 0,
        }
    }
    /// Original relative names, captured once per operation. The host retains
    /// its immutable input guards. Refusal never retries the filesystem.
    pub fn admitted(read: &'a mut ReadBytes<'a>) -> Self {
        Self {
            read: Some(read),
            snapshots: BTreeMap::new(),
            total: 0,
        }
    }
    pub fn is_admitted(&self) -> bool {
        self.read.is_some()
    }
    pub fn names(&self) -> impl Iterator<Item = &Path> {
        self.snapshots.keys().map(PathBuf::as_path)
    }
    pub fn read(&mut self, path: &Path) -> Result<Vec<u8>> {
        let Some(read) = &mut self.read else {
            return Ok(fs::read(path)?);
        };
        ensure!(
            !path.as_os_str().is_empty()
                && path.components().all(|c| matches!(c, Component::Normal(_))),
            "admitted score name must be relative without aliases"
        );
        if let Some(raw) = self.snapshots.get(path) {
            return Ok(raw.clone());
        }
        ensure!(
            self.snapshots.len() < 32,
            "admitted score object count exceeds profile"
        );
        let raw = read(path)?;
        ensure!(
            raw.len() <= 1024 * 1024,
            "admitted score object exceeds byte profile"
        );
        let total = self
            .total
            .checked_add(raw.len())
            .ok_or_else(|| anyhow::anyhow!("admitted score byte overflow"))?;
        ensure!(
            total <= 16 * 1024 * 1024,
            "admitted score total exceeds byte profile"
        );
        self.total = total;
        self.snapshots.insert(path.into(), raw.clone());
        Ok(raw)
    }
    pub fn text(&mut self, path: &Path) -> Result<String> {
        Ok(String::from_utf8(self.read(path)?)?)
    }
    pub fn sha256(&mut self, path: &Path) -> Result<String> {
        if self.is_admitted() {
            Ok(crate::hash::sha256_bytes(&self.read(path)?))
        } else {
            crate::hash::sha256_path(path)
        }
    }
    pub fn len(&mut self, path: &Path) -> Result<u64> {
        if self.is_admitted() {
            Ok(self.read(path)?.len() as u64)
        } else {
            Ok(fs::metadata(path)?.len())
        }
    }
}
