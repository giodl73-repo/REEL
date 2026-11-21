//! Read-only existing-cache layout adapter. Disposable links are consumer-owned.
//! Layout/alias behavior adapted from REEL e8fb31c (MIT; see NOTICE.md).
use crate::protocol::{AgentError, Download, DownloadBackend, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use tig_store::{
    Key,
    ledger::{CapacityLedger, Checkpoint, JournalRef, LedgerError},
};

pub use tig_store::existing::Layout;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub ledger_root: PathBuf,
    pub checkpoint: Checkpoint,
    pub job: JournalRef,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub root: PathBuf,
    pub layout: Layout,
    pub enabled: bool,
    pub handoff_directory: PathBuf,
    pub admission: Admission,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub authority_root: Object,
    pub authority_nodes: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_checkpoint: Option<tig_store::metadata::Checkpoint>,
    #[serde(default = "default_authority_entries")]
    pub max_authority_entries: u32,
    pub places: Vec<Place>,
    pub max_object_bytes: u64,
    pub max_requests: u16,
    pub metadata_window: u64,
    pub min_free_bytes: u64,
}
fn default_authority_entries() -> u32 {
    500_000
}
impl Profile {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let value = identity::parse_json(bytes).map_err(|_| AgentError::InvalidProfile)?;
        let p: Self = serde_json::from_value(value).map_err(|_| AgentError::InvalidProfile)?;
        p.validate()?;
        Ok(p)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        identity::canonical_json(
            &serde_json::to_value(self).map_err(|_| AgentError::InvalidProfile)?,
        )
        .map_err(|_| AgentError::InvalidProfile)
    }
    fn validate(&self) -> Result<()> {
        if self.schema != "tig.lfs-download.design-1"
            || self.max_authority_entries == 0
            || self.max_authority_entries > 2_000_000
            || self.places.is_empty()
            || self.places.len() > 16
            || self.max_object_bytes == 0
            || self.max_object_bytes > 64 * 1024 * 1024 * 1024
            || self.max_requests == 0
            || self.max_requests > 1024
            || self.metadata_window < 65_536
            || self.metadata_window > 4 * 1024 * 1024
            || self.min_free_bytes < 65_536
            || self.authority_root.bytes > 65_536
        {
            return Err(AgentError::InvalidProfile);
        }
        absolute(&self.authority_nodes)?;
        for p in &self.places {
            absolute(&p.root)?;
            absolute(&p.handoff_directory)?;
            absolute(&p.admission.ledger_root)?;
        }
        Ok(())
    }
}
fn absolute(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(AgentError::InvalidProfile);
    }
    Ok(())
}
fn directory(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(AgentError::Integrity);
    }
    Ok(())
}
fn regular(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(AgentError::Integrity);
    }
    Ok(())
}
pub struct ReadNodes {
    root: PathBuf,
}
impl ReadNodes {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}
impl NodeStore for ReadNodes {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        if object.bytes > 65_536 {
            return Err(TreeError::Budget);
        }
        directory(&self.root).map_err(|_| TreeError::Store("metadata root unavailable".into()))?;
        let path = self.root.join(object.sha256.as_str());
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(TreeError::Store("metadata read failed".into())),
            Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
                return Err(TreeError::Integrity);
            }
            _ => (),
        }
        let file = File::open(path).map_err(|_| TreeError::Store("metadata read failed".into()))?;
        if file.metadata().map_err(|_| TreeError::Integrity)?.len() != object.bytes {
            return Err(TreeError::Integrity);
        }
        let mut bytes = Vec::new();
        file.take(object.bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| TreeError::Integrity)?;
        if Object::from_bytes(&bytes) != *object {
            return Err(TreeError::Integrity);
        }
        Ok(Some(bytes))
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Store("read-only metadata adapter".into()))
    }
}
pub struct CacheBackend {
    profile: Profile,
    nonce: Digest,
    requests: u16,
}
enum AuthorityNodes {
    Files(ReadNodes),
    Sqlite(tig_store::metadata::MetadataReader),
}
impl NodeStore for AuthorityNodes {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        match self {
            Self::Files(s) => s.get(o),
            Self::Sqlite(s) => s.get(o),
        }
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Store("read-only authority".into()))
    }
}
impl CacheBackend {
    fn nodes(&self) -> Result<AuthorityNodes> {
        match &self.profile.authority_checkpoint {
            Some(minimum) => {
                tig_store::metadata::MetadataReader::open(&self.profile.authority_nodes, minimum)
                    .map(AuthorityNodes::Sqlite)
                    .map_err(|_| AgentError::Authority)
            }
            None => Ok(AuthorityNodes::Files(ReadNodes::new(
                self.profile.authority_nodes.clone(),
            ))),
        }
    }
    pub fn new(profile: Profile) -> Result<Self> {
        profile.validate()?;
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(|_| AgentError::Io)?;
        Ok(Self {
            profile,
            nonce: Digest::blob(&nonce),
            requests: 0,
        })
    }
    fn authorized(&self, object: &Object) -> Result<()> {
        let mut nodes = self.nodes()?;
        let path = LogicalPath::parse(object.sha256.as_str()).map_err(|_| AgentError::Authority)?;
        let limits = TreeLimits {
            max_entries: self.profile.max_authority_entries as usize,
            max_reads: 128,
            max_read_bytes: 8 * 1024 * 1024,
            ..TreeLimits::default()
        };
        let (found, _) = Tree::from_root(self.profile.authority_root.clone())
            .get(&path, &mut nodes, limits)
            .map_err(|_| AgentError::Authority)?;
        if found.as_ref() != Some(object) {
            return Err(AgentError::Authority);
        }
        Ok(())
    }
    fn candidate(place: &Place, object: &Object) -> Result<Option<PathBuf>> {
        let mut dirs = vec![place.root.clone()];
        let mut path = place.root.clone();
        for c in place.layout.components(&object.sha256) {
            path.push(c);
            dirs.push(path.clone());
        }
        for dir in dirs {
            let Some(m) = optional_metadata(&dir)? else {
                return Ok(None);
            };
            if !m.is_dir() || m.file_type().is_symlink() {
                return Err(AgentError::Integrity);
            }
        }
        let path = path.join(object.sha256.as_str());
        let Some(m) = optional_metadata(&path)? else {
            return Ok(None);
        };
        if !m.is_file() || m.file_type().is_symlink() || m.len() != object.bytes {
            return Err(AgentError::Integrity);
        }
        Ok(Some(path))
    }
}
fn optional_metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(m) => Ok(Some(m)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err(AgentError::Io),
    }
}
fn key(s: &str) -> Result<Key> {
    Key::parse(s).map_err(|_| AgentError::InvalidProfile)
}
fn map_ledger(_: LedgerError) -> AgentError {
    AgentError::Capacity
}
fn admit(place: &Place, op: &str, window: u64, floor: u64) -> Result<JournalRef> {
    admit_with(place, op, window, floor, |_, _| Ok(()))
}
fn admit_with(
    place: &Place,
    op: &str,
    window: u64,
    floor: u64,
    mut after_probe: impl FnMut(&mut CapacityLedger, usize) -> tig_store::ledger::Result<()>,
) -> Result<JournalRef> {
    for attempt in 0..8 {
        let result = (|| {
            let mut l =
                CapacityLedger::open(&place.admission.ledger_root, &place.admission.checkpoint)?;
            let probe = l.begin_probe(Key::parse(&format!("probe-{op}-{attempt}"))?)?;
            after_probe(&mut l, attempt)?;
            let free =
                fs2::available_space(&place.handoff_directory).map_err(|_| LedgerError::Budget)?;
            if free < floor {
                return Err(LedgerError::Budget);
            }
            l.observe(
                Key::parse(&format!("sample-{op}-{attempt}"))?,
                probe.reference(),
                free,
            )?;
            Ok(l.begin_allocation(
                Key::parse(op)?,
                &place.admission.job,
                Key::parse(op)?,
                window,
            )?
            .reference()
            .clone())
        })();
        match result {
            Ok(r) => return Ok(r),
            // Another observer can supersede this probe between journal steps.
            // Re-probe/re-sample; never spend the rejected or expired sample.
            Err(
                LedgerError::Busy
                | LedgerError::NeedsProbe
                | LedgerError::Capacity(
                    tig_core::capacity::CapacityError::Stale
                    | tig_core::capacity::CapacityError::UnknownObservation,
                ),
            ) => std::thread::sleep(Duration::from_millis(2 << attempt)),
            Err(e) => return Err(map_ledger(e)),
        }
    }
    Err(AgentError::Capacity)
}

fn settle(place: &Place, op: &str, allocation: &JournalRef) -> Result<()> {
    for attempt in 0..8 {
        let result =
            CapacityLedger::open(&place.admission.ledger_root, &place.admission.checkpoint)
                .and_then(|mut l| {
                    l.complete_allocation(
                        key(&format!("done-{op}")).map_err(|_| LedgerError::Budget)?,
                        allocation,
                        0,
                    )
                });
        match result {
            Ok(_) => return Ok(()),
            Err(LedgerError::Busy) => std::thread::sleep(Duration::from_millis(2 << attempt)),
            Err(e) => return Err(map_ledger(e)),
        }
    }
    Err(AgentError::Capacity)
}
fn verify(path: &Path, object: &Object) -> Result<()> {
    regular(path)?;
    let file = File::open(path)?;
    if file.metadata()?.len() != object.bytes {
        return Err(AgentError::Integrity);
    }
    let mut file = file.take(object.bytes + 1);
    let mut hash = Sha256::new();
    let mut buffer = [0; 65_536];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total = total.checked_add(n as u64).ok_or(AgentError::Limit)?;
        if total > object.bytes {
            return Err(AgentError::Integrity);
        }
        hash.update(&buffer[..n]);
    }
    if total != object.bytes || Digest::from_sha256(hash.finalize().into()) != object.sha256 {
        return Err(AgentError::Integrity);
    }
    Ok(())
}
impl DownloadBackend for CacheBackend {
    fn initialize(&mut self) -> Result<()> {
        let nodes = self.nodes()?;
        nodes
            .get(&self.profile.authority_root)
            .map_err(|_| AgentError::Authority)?
            .ok_or(AgentError::Authority)?;
        Ok(())
    }
    fn download(&mut self, object: &Object) -> Result<Download> {
        if self.requests >= self.profile.max_requests
            || object.bytes > self.profile.max_object_bytes
        {
            return Err(AgentError::Limit);
        }
        self.requests += 1;
        self.authorized(object)?;
        let op = format!(
            "lfs-{}-{}-{}",
            &self.nonce.as_str()[..24],
            self.requests,
            object.sha256.as_str()
        );
        for place in self.profile.places.iter().filter(|p| p.enabled) {
            let Some(source) = Self::candidate(place, object)? else {
                continue;
            };
            directory(&place.handoff_directory)?;
            let allocation = admit(
                place,
                &op,
                self.profile.metadata_window,
                self.profile.min_free_bytes,
            )?;
            let alias = place.handoff_directory.join(&op);
            fs::hard_link(source, &alias)?;
            let download = Download::from_owned_path(alias.clone());
            verify(&alias, object)?;
            settle(place, &op, &allocation)?;
            return Ok(download);
        }
        Err(AgentError::Unavailable)
    }
}

#[cfg(test)]
mod admission_race_tests {
    use super::*;
    #[test]
    fn superseded_probe_is_resampled_without_a_global_lock() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("ledger");
        let mut ledger =
            CapacityLedger::create(&root, key("volume").unwrap(), key("epoch").unwrap(), 0)
                .unwrap();
        let checkpoint = ledger.checkpoint();
        let probe = ledger.begin_probe(key("seed-probe").unwrap()).unwrap();
        ledger
            .observe(
                key("seed-sample").unwrap(),
                probe.reference(),
                1024 * 1024 * 1024,
            )
            .unwrap();
        let job = ledger
            .reserve(key("job").unwrap(), key("owner").unwrap(), 1024 * 1024)
            .unwrap()
            .reference()
            .clone();
        let place = Place {
            root: temp.path().to_owned(),
            layout: Layout::ReelCas,
            enabled: true,
            handoff_directory: temp.path().to_owned(),
            admission: Admission {
                ledger_root: root.clone(),
                checkpoint: checkpoint.clone(),
                job,
            },
        };
        let mut attempts = Vec::new();
        let allocation = admit_with(&place, "test-admission", 65_536, 65_536, |_, attempt| {
            attempts.push(attempt);
            if attempt == 0 {
                let mut competitor = CapacityLedger::open(&root, &checkpoint)?;
                let probe = competitor.begin_probe(key("competing-probe").unwrap())?;
                competitor.observe(
                    key("competing-sample").unwrap(),
                    probe.reference(),
                    1024 * 1024 * 1024,
                )?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(attempts, vec![0, 1]);
        settle(&place, "test-admission", &allocation).unwrap();
    }
}
