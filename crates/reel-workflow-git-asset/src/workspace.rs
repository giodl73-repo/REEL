//! Explicit owned input copies using existing directory guards and ByteSource.
//! No cache enrollment, directory census, overwrite, cleanup or task completion.
use crate::{Preview, Snapshot, error};
use git_asset::{restore::ordinary, source::GitId};
use git_asset_core::{LogicalPath, Object};
use git_asset_store::existing::{self, ByteSource};
use reel_assets::jobs::{WorkspaceDirectory, open_workspace_directory};
use reel_workflow::{Artifact, Error, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
const MAX_CONTROL: usize = 256 * 1024;
const ATTEMPTS: usize = 4;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub logical_bytes: u64,
    pub peak_file_data: u64,
    pub millis: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema: String,
    preview: Preview,
    bindings: BTreeMap<String, String>,
    budget: Budget,
}
pub struct Preparation {
    wire: Wire,
    paths: BTreeMap<String, LogicalPath>,
    directories: BTreeSet<String>,
    encoded: Vec<u8>,
    peak: u64,
}
fn io_error(e: io::Error) -> Error {
    error(
        if matches!(e.raw_os_error(), Some(32 | 33)) || e.kind() == io::ErrorKind::WouldBlock {
            ErrorKind::Busy
        } else {
            match e.kind() {
                io::ErrorKind::NotFound => ErrorKind::Missing,
                io::ErrorKind::AlreadyExists => ErrorKind::Conflict,
                io::ErrorKind::PermissionDenied => ErrorKind::Denied,
                io::ErrorKind::StorageFull | io::ErrorKind::OutOfMemory => ErrorKind::Budget,
                _ => ErrorKind::NeedsRecovery,
            }
        },
        "public-git-asset-workspace-io",
    )
}
fn scope_error(_: impl std::fmt::Display) -> Error {
    error(ErrorKind::Denied, "public-git-asset-workspace-namespace")
}
fn source_error(e: existing::Error) -> Error {
    error(
        match e {
            existing::Error::Missing => ErrorKind::Missing,
            existing::Error::Integrity => ErrorKind::Integrity,
            existing::Error::Budget => ErrorKind::Budget,
            existing::Error::Busy => ErrorKind::Busy,
            existing::Error::Unsupported => ErrorKind::Unsupported,
            existing::Error::Denied | existing::Error::Invalid => ErrorKind::Denied,
            _ => ErrorKind::NeedsRecovery,
        },
        "public-git-asset-workspace-source",
    )
}
fn portable(name: &str) -> Result<LogicalPath> {
    if name.len() > 200
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_./".contains(&c))
    {
        return Err(error(ErrorKind::Invalid, "public-git-asset-workspace-path"));
    }
    LogicalPath::parse(name)
        .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-workspace-path"))
}
impl Preparation {
    pub fn capture(
        snapshot: &Snapshot,
        preview: &Preview,
        bindings: BTreeMap<String, String>,
        budget: Budget,
    ) -> Result<Self> {
        snapshot.verify_preview(preview, budget.logical_bytes)?;
        Self::checked(Wire {
            schema: "reel.public-input-preparation.v1".into(),
            preview: preview.clone(),
            bindings,
            budget,
        })
    }
    fn checked(wire: Wire) -> Result<Self> {
        if wire.schema != "reel.public-input-preparation.v1" {
            return Err(error(
                ErrorKind::Invalid,
                "public-git-asset-workspace-control",
            ));
        }
        if !(1..=300_000).contains(&wire.budget.millis) {
            return Err(error(ErrorKind::Budget, "public-git-asset-workspace-time"));
        }
        wire.preview.encode()?;
        GitId::parse(&wire.preview.commit).map_err(crate::source_error)?;
        if wire.bindings.len() != wire.preview.inputs.len() {
            return Err(error(
                ErrorKind::Conflict,
                "public-git-asset-workspace-input-set",
            ));
        }
        let mut total = 0u64;
        let mut paths = BTreeMap::new();
        let mut directories = BTreeSet::new();
        let mut names = BTreeMap::new();
        for input in &wire.preview.inputs {
            reel_publication::check_artifact(&input.artifact).map_err(Error::from)?;
            if input.artifact.sha256 != input.object.sha256.as_str()
                || input.artifact.bytes != input.object.bytes
                || input.object.bytes > 64 * 1024 * 1024 * 1024
            {
                return Err(error(
                    ErrorKind::Integrity,
                    "public-git-asset-workspace-input",
                ));
            }
            total = total
                .checked_add(input.object.bytes)
                .ok_or_else(|| error(ErrorKind::Budget, "public-git-asset-workspace-bytes"))?;
            let name = wire
                .bindings
                .get(&input.artifact.logical_id)
                .ok_or_else(|| error(ErrorKind::Missing, "public-git-asset-workspace-binding"))?;
            let path = portable(name)?;
            let mut prefix = String::new();
            let count = path.components().count();
            for (index, component) in path.components().enumerate() {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(component);
                let directory = index + 1 < count;
                if let Some((old, old_dir)) = names.get(&prefix.to_ascii_lowercase()) {
                    if old != &prefix || old_dir != &directory || !directory {
                        return Err(error(
                            ErrorKind::Conflict,
                            "public-git-asset-workspace-collision",
                        ));
                    }
                } else {
                    names.insert(prefix.to_ascii_lowercase(), (prefix.clone(), directory));
                }
                if directory {
                    directories.insert(prefix.clone());
                }
            }
            if paths
                .insert(input.artifact.logical_id.clone(), path)
                .is_some()
            {
                return Err(error(
                    ErrorKind::Conflict,
                    "public-git-asset-workspace-input-set",
                ));
            }
        }
        if total != wire.preview.logical_bytes
            || total > wire.budget.logical_bytes
            || total > 1024 * 1024 * 1024 * 1024
            || directories.len() > 512
        {
            return Err(error(
                ErrorKind::Budget,
                "public-git-asset-workspace-budget",
            ));
        }
        let encoded = serde_json::to_vec(&wire)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-workspace-control"))?;
        if encoded.len() > MAX_CONTROL {
            return Err(error(
                ErrorKind::Budget,
                "public-git-asset-workspace-control",
            ));
        }
        let peak = total
            .checked_mul((ATTEMPTS + 1) as u64)
            .and_then(|n| n.checked_add(encoded.len() as u64 + 16384))
            .ok_or_else(|| error(ErrorKind::Budget, "public-git-asset-workspace-peak"))?;
        if peak > wire.budget.peak_file_data {
            return Err(error(ErrorKind::Budget, "public-git-asset-workspace-peak"));
        }
        Ok(Self {
            wire,
            paths,
            directories,
            encoded,
            peak,
        })
    }
    pub fn control(&self) -> &[u8] {
        &self.encoded
    }
    /// Only the ORIGINAL independently pinned admitted preparation control.
    pub fn restore(raw: &[u8], expected: &Object) -> Result<Self> {
        if raw.len() > MAX_CONTROL {
            return Err(error(
                ErrorKind::Budget,
                "public-git-asset-workspace-control",
            ));
        }
        if &Object::from_bytes(raw) != expected {
            return Err(error(
                ErrorKind::Integrity,
                "public-git-asset-workspace-control",
            ));
        }
        let wire = serde_json::from_slice(raw)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-workspace-control"))?;
        let plan = Self::checked(wire)?;
        if plan.encoded != raw {
            return Err(error(
                ErrorKind::Invalid,
                "public-git-asset-workspace-control-encoding",
            ));
        }
        Ok(plan)
    }
    pub fn logical_bytes(&self) -> u64 {
        self.wire.preview.logical_bytes
    }
    pub fn peak_file_data(&self) -> u64 {
        self.peak
    }
    fn artifact(&self, id: &str) -> &Artifact {
        &self
            .wire
            .preview
            .inputs
            .iter()
            .find(|i| i.artifact.logical_id == id)
            .expect("checked exact set")
            .artifact
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    schema: String,
    parent: PathBuf,
    name: String,
    parent_identity: String,
    root_identity: String,
    plan: Object,
}
#[derive(Clone, Serialize)]
pub struct Ticket {
    owner: Owner,
    marker: Object,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketWire {
    owner: Owner,
    marker: Object,
}
impl Ticket {
    pub fn encode(&self) -> Result<Vec<u8>> {
        let raw = serde_json::to_vec(self)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-workspace-ticket"))?;
        if raw.len() > 16384 {
            return Err(error(
                ErrorKind::Budget,
                "public-git-asset-workspace-ticket",
            ));
        }
        Ok(raw)
    }
    pub fn decode(raw: &[u8], expected: &Object) -> Result<Self> {
        if raw.len() > 16384 {
            return Err(error(
                ErrorKind::Budget,
                "public-git-asset-workspace-ticket",
            ));
        }
        if &Object::from_bytes(raw) != expected {
            return Err(error(
                ErrorKind::Integrity,
                "public-git-asset-workspace-ticket",
            ));
        }
        let wire: TicketWire = serde_json::from_slice(raw)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-workspace-ticket"))?;
        Ok(Self {
            owner: wire.owner,
            marker: wire.marker,
        })
    }
}
#[derive(Serialize, Debug)]
pub struct Summary {
    pub files: usize,
    pub logical_bytes: u64,
    pub peak_file_data: u64,
    pub already_complete: bool,
}
pub struct Workspace {
    ticket: Ticket,
    plan: Preparation,
    root: WorkspaceDirectory,
    _directories: Vec<WorkspaceDirectory>,
    files: BTreeMap<String, File>,
    _gate: File,
    _marker: File,
}
fn supported() -> Result<()> {
    if !cfg!(windows) {
        Err(error(
            ErrorKind::Unsupported,
            "public-git-asset-workspace-platform",
        ))
    } else {
        Ok(())
    }
}
fn read_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options.open(path).map_err(io_error)?;
    let m = file.metadata().map_err(io_error)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(error(ErrorKind::Denied, "public-git-asset-workspace-link"));
        }
    }
    if !m.is_file() {
        return Err(error(ErrorKind::Denied, "public-git-asset-workspace-file"));
    }
    Ok(file)
}
fn observed(file: &mut File, object: &Object, deadline: std::time::Instant) -> Result<()> {
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    transfer(file, object, None, deadline)
}
fn transfer(
    input: &mut impl Read,
    object: &Object,
    mut output: Option<&mut File>,
    deadline: std::time::Instant,
) -> Result<()> {
    let mut hash = Sha256::new();
    let mut length = 0u64;
    let mut bytes = [0u8; 65536];
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(error(ErrorKind::Budget, "public-git-asset-workspace-time"));
        }
        let count = match input.read(&mut bytes) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io_error(e)),
        };
        if count == 0 {
            break;
        }
        length = length
            .checked_add(count as u64)
            .filter(|n| *n <= object.bytes)
            .ok_or_else(|| error(ErrorKind::Integrity, "public-git-asset-workspace-content"))?;
        hash.update(&bytes[..count]);
        if let Some(file) = output.as_mut() {
            file.write_all(&bytes[..count]).map_err(io_error)?;
        }
    }
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if length != object.bytes || digest != object.sha256.as_str() {
        return Err(error(
            ErrorKind::Integrity,
            "public-git-asset-workspace-content",
        ));
    }
    Ok(())
}
impl Workspace {
    /// Explicit new owned scratch only. Persist the ticket before materializing.
    pub fn create(parent: &Path, name: &str, plan: Preparation) -> Result<(Ticket, Self)> {
        supported()?;
        let name = portable(name)?;
        if name.components().count() != 1 {
            return Err(error(ErrorKind::Invalid, "public-git-asset-workspace-name"));
        }
        let parent = open_workspace_directory(parent).map_err(scope_error)?;
        let path = parent.path().join(name.as_str());
        fs::create_dir(&path).map_err(io_error)?;
        let root = open_workspace_directory(&path).map_err(scope_error)?;
        let owner = Owner {
            schema: "reel.public-input-workspace.v1".into(),
            parent: parent.path().into(),
            name: name.as_str().into(),
            parent_identity: parent.identity().into(),
            root_identity: root.identity().into(),
            plan: Object::from_bytes(plan.control()),
        };
        let marker = serde_json::to_vec(&owner)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-workspace-owner"))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path.join("owner.json"))
            .map_err(io_error)?;
        file.write_all(&marker).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path.join("prepare.lock"))
            .map_err(io_error)?;
        let mut created = Vec::new();
        for d in ["inputs", "parts"] {
            let directory = path.join(d);
            fs::create_dir(&directory).map_err(io_error)?;
            created.push(open_workspace_directory(&directory).map_err(scope_error)?);
        }
        for d in &plan.directories {
            let directory = path.join("inputs").join(d);
            fs::create_dir(&directory).map_err(io_error)?;
            created.push(open_workspace_directory(&directory).map_err(scope_error)?);
        }
        let ticket = Ticket {
            owner,
            marker: Object::from_bytes(&marker),
        };
        drop(created);
        drop(root);
        drop(parent);
        let workspace = Self::reopen(&ticket, plan)?;
        Ok((ticket, workspace))
    }
    pub fn reopen(ticket: &Ticket, plan: Preparation) -> Result<Self> {
        supported()?;
        let name = portable(&ticket.owner.name)?;
        if name.components().count() != 1
            || ticket.owner.schema != "reel.public-input-workspace.v1"
            || ticket.owner.plan != Object::from_bytes(plan.control())
        {
            return Err(error(
                ErrorKind::Conflict,
                "public-git-asset-workspace-owner",
            ));
        }
        let parent = open_workspace_directory(&ticket.owner.parent).map_err(scope_error)?;
        let root =
            open_workspace_directory(&parent.path().join(name.as_str())).map_err(scope_error)?;
        if parent.identity() != ticket.owner.parent_identity
            || root.identity() != ticket.owner.root_identity
        {
            return Err(error(
                ErrorKind::Stale,
                "public-git-asset-workspace-replaced",
            ));
        }
        let expected = serde_json::to_vec(&ticket.owner)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-workspace-owner"))?;
        if expected.len() > 16384 || ticket.marker != Object::from_bytes(&expected) {
            return Err(error(
                ErrorKind::Integrity,
                "public-git-asset-workspace-owner",
            ));
        }
        let mut marker = read_file(&root.path().join("owner.json"))?;
        observed(
            &mut marker,
            &ticket.marker,
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        )?;
        let path = root.path().join("prepare.lock");
        let metadata = ordinary(&path).map_err(scope_error)?;
        if !metadata.is_file() || metadata.len() != 0 {
            return Err(error(ErrorKind::Denied, "public-git-asset-workspace-gate"));
        }
        let mut opts = OpenOptions::new();
        opts.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            opts.share_mode(3).custom_flags(0x0020_0000);
        }
        let gate = opts.open(path).map_err(io_error)?;
        let gm = gate.metadata().map_err(io_error)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if gm.file_attributes() & 0x400 != 0 {
                return Err(error(ErrorKind::Denied, "public-git-asset-workspace-gate"));
            }
        }
        if !gm.is_file() || gm.len() != 0 {
            return Err(error(ErrorKind::Denied, "public-git-asset-workspace-gate"));
        }
        gate.try_lock().map_err(|e| match e {
            fs::TryLockError::WouldBlock => {
                error(ErrorKind::Busy, "public-git-asset-workspace-busy")
            }
            fs::TryLockError::Error(e) => io_error(e),
        })?;
        let mut directories = vec![parent];
        for d in ["inputs", "parts"] {
            directories.push(open_workspace_directory(&root.path().join(d)).map_err(scope_error)?);
        }
        for d in &plan.directories {
            directories.push(
                open_workspace_directory(&root.path().join("inputs").join(d))
                    .map_err(scope_error)?,
            );
        }
        Ok(Self {
            ticket: ticket.clone(),
            plan,
            root,
            _directories: directories,
            files: BTreeMap::new(),
            _gate: gate,
            _marker: marker,
        })
    }
    pub fn path(&self) -> &Path {
        self.root.path()
    }
    pub fn ticket(&self) -> &Ticket {
        &self.ticket
    }
    pub fn input_path(&self, id: &str) -> Result<PathBuf> {
        let name = self
            .plan
            .paths
            .get(id)
            .ok_or_else(|| error(ErrorKind::Missing, "public-git-asset-workspace-input"))?;
        Ok(self.root.path().join("inputs").join(name.as_str()))
    }
    fn existing(
        &mut self,
        id: &str,
        object: &Object,
        deadline: std::time::Instant,
    ) -> Result<bool> {
        if let Some(file) = self.files.get_mut(id) {
            observed(file, object, deadline)?;
            return Ok(true);
        }
        let path = self.input_path(id)?;
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(io_error(e)),
            Ok(_) => {}
        }
        let mut file = read_file(&path)?;
        observed(&mut file, object, deadline)?;
        self.files.insert(id.into(), file);
        Ok(true)
    }
    pub fn verify(&mut self) -> Result<Summary> {
        self.verify_at(
            std::time::Instant::now()
                + std::time::Duration::from_millis(self.plan.wire.budget.millis),
        )
    }
    fn verify_at(&mut self, deadline: std::time::Instant) -> Result<Summary> {
        for id in self.plan.paths.keys().cloned().collect::<Vec<_>>() {
            let a = self.plan.artifact(&id);
            let o = Object {
                sha256: git_asset_core::Digest::parse(&a.sha256).expect("checked"),
                bytes: a.bytes,
            };
            if !self.existing(&id, &o, deadline)? {
                return Err(error(
                    ErrorKind::Missing,
                    "public-git-asset-workspace-input",
                ));
            }
        }
        Ok(Summary {
            files: self.plan.paths.len(),
            logical_bytes: self.plan.logical_bytes(),
            peak_file_data: self.plan.peak,
            already_complete: true,
        })
    }
    pub fn materialize<S: ByteSource>(&mut self, source: &mut S) -> Result<Summary> {
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(self.plan.wire.budget.millis);
        let mut already_complete = true;
        for (index, id) in self
            .plan
            .paths
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
        {
            let a = self.plan.artifact(&id);
            let object = Object {
                sha256: git_asset_core::Digest::parse(&a.sha256).expect("checked"),
                bytes: a.bytes,
            };
            if self.existing(&id, &object, deadline)? {
                continue;
            }
            already_complete = false;
            let mut complete = None;
            for attempt in 0..ATTEMPTS {
                let part = self
                    .root
                    .path()
                    .join("parts")
                    .join(format!("{index:03}-{attempt}.part"));
                match fs::symlink_metadata(&part) {
                    Ok(_) => {
                        let mut file = read_file(&part)?;
                        let size = file.metadata().map_err(io_error)?.len();
                        if size > object.bytes {
                            return Err(error(
                                ErrorKind::Integrity,
                                "public-git-asset-workspace-part",
                            ));
                        }
                        if size < object.bytes {
                            continue;
                        }
                        observed(&mut file, &object, deadline)?;
                        complete = Some((part, file));
                        break;
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(io_error(e)),
                }
                let mut input = source.open_verified(&object).map_err(source_error)?;
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    options.share_mode(0).custom_flags(0x0020_0000);
                }
                let mut output = options.open(&part).map_err(io_error)?;
                transfer(&mut input, &object, Some(&mut output), deadline)?;
                output.sync_all().map_err(io_error)?;
                drop(output);
                let mut file = read_file(&part)?;
                observed(&mut file, &object, deadline)?;
                complete = Some((part, file));
                break;
            }
            let (part, file) = complete.ok_or_else(|| {
                error(
                    ErrorKind::NeedsRecovery,
                    "public-git-asset-workspace-part-slots",
                )
            })?;
            fs::hard_link(&part, self.input_path(&id)?).map_err(io_error)?;
            drop(file);
            if !self.existing(&id, &object, deadline)? {
                return Err(error(
                    ErrorKind::NeedsRecovery,
                    "public-git-asset-workspace-publication",
                ));
            }
        }
        let mut summary = self.verify_at(deadline)?;
        summary.already_complete = already_complete;
        Ok(summary)
    }
}

#[cfg(test)]
#[path = "workspace/tests.rs"]
mod tests;
