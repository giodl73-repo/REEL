//! Bounded directory identity snapshots; never payload custody or deletion authority.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceEntry {
    pub identity: String,
    pub directory: bool,
    pub bytes: u64,
    pub modified: String,
}
impl WorkspaceEntry {
    pub fn same_survivor(&self, original: &Self) -> bool {
        self.identity == original.identity
            && self.directory == original.directory
            && (self.directory
                || (self.bytes == original.bytes && self.modified == original.modified))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshot {
    pub root: WorkspaceEntry,
    pub entries: BTreeMap<String, WorkspaceEntry>,
}
impl WorkspaceSnapshot {
    pub fn is_survivor_of(&self, original: &Self) -> bool {
        self.root.same_survivor(&original.root)
            && self.entries.iter().all(|(name, entry)| {
                original
                    .entries
                    .get(name)
                    .is_some_and(|old| entry.same_survivor(old))
            })
    }
}

#[cfg(windows)]
fn entry(path: &Path) -> Result<WorkspaceEntry> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    // Attributes only; open the reparse point itself instead of following it.
    let file = OpenOptions::new()
        .read(true)
        .access_mode(0x80)
        .share_mode(7)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if info.dwFileAttributes & 0x400 != 0 {
        return Err("Workspace snapshot rejects reparse points".into());
    }
    let directory = info.dwFileAttributes & 0x10 != 0;
    Ok(WorkspaceEntry {
        identity: format!(
            "{:08x}:{:08x}{:08x}:{:08x}{:08x}",
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
            info.ftCreationTime.dwHighDateTime,
            info.ftCreationTime.dwLowDateTime
        ),
        directory,
        bytes: if directory {
            0
        } else {
            u64::from(info.nFileSizeHigh) << 32 | u64::from(info.nFileSizeLow)
        },
        modified: format!(
            "{:08x}{:08x}",
            info.ftLastWriteTime.dwHighDateTime, info.ftLastWriteTime.dwLowDateTime
        ),
    })
}
#[cfg(not(windows))]
fn entry(_: &Path) -> Result<WorkspaceEntry> {
    Err("Workspace identities currently require Windows".into())
}

fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    entries: &mut BTreeMap<String, WorkspaceEntry>,
) -> Result<()> {
    if depth > 16 {
        return Err("Workspace snapshot depth exceeded".into());
    }
    for item in fs::read_dir(dir)? {
        let path = item?.path();
        let observed = entry(&path)?;
        let name = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("Workspace path is not Unicode")?
            .replace('\\', "/");
        let directory = observed.directory;
        entries.insert(name, observed);
        if entries.len() > 10_000 {
            return Err("Workspace snapshot entry limit exceeded".into());
        }
        if directory {
            walk(root, &path, depth + 1, entries)?;
        }
    }
    Ok(())
}

pub fn snapshot_owned_workspace(
    cfg: &crate::Config,
    cache: &Path,
    workspace: &Path,
) -> Result<WorkspaceSnapshot> {
    let canonical = lifecycle::checked_workspace(cfg, cache, workspace)?;
    let root = entry(&canonical)?;
    if !root.directory {
        return Err("Workspace snapshot root is not a directory".into());
    }
    let mut entries = BTreeMap::new();
    walk(&canonical, &canonical, 0, &mut entries)?;
    Ok(WorkspaceSnapshot { root, entries })
}
