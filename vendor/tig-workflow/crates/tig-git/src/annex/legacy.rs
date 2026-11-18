//! Read-only external special remote v2. Source and staging scopes are enrolled.
use super::identity::Key;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufRead, Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};
use tig_store::existing::{ByteSource, ReadCache};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Source {
    Existing {
        profile: tig_store::existing::Profile,
    },
    Native {
        root: PathBuf,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub source: Source,
    pub staging: Vec<PathBuf>,
    pub object_bytes: u64,
    pub total_bytes: u64,
    pub requests: u32,
    pub transfer_millis: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Present,
    Missing,
    Unknown,
}

enum Reader {
    Existing(ReadCache),
    Native(PathBuf),
}
pub struct Backend {
    profile: Profile,
    reader: Reader,
    calls: u32,
    bytes: u64,
}
impl Backend {
    pub fn new(profile: Profile) -> io::Result<Self> {
        if profile.staging.is_empty()
            || profile.staging.len() > 8
            || profile.staging.iter().any(|p| !absolute(p))
            || !(1..=65_536).contains(&profile.requests)
            || !(1..=64 * 1024 * 1024 * 1024).contains(&profile.object_bytes)
            || profile.total_bytes < profile.object_bytes
            || profile.total_bytes > 1024 * 1024 * 1024 * 1024
            || !(1..=300_000).contains(&profile.transfer_millis)
        {
            return Err(failure());
        }
        let reader = match &profile.source {
            Source::Existing { profile } => {
                Reader::Existing(ReadCache::new(profile.clone()).map_err(|_| failure())?)
            }
            Source::Native { root } if absolute(root) => Reader::Native(root.clone()),
            _ => return Err(failure()),
        };
        Ok(Self {
            profile,
            reader,
            calls: 0,
            bytes: 0,
        })
    }
    fn open(&mut self, key: &Key) -> Result<Box<dyn Read>, Availability> {
        let object = key.object();
        if self.calls >= self.profile.requests
            || object.bytes > self.profile.object_bytes
            || self
                .bytes
                .checked_add(object.bytes)
                .is_none_or(|b| b > self.profile.total_bytes)
        {
            return Err(Availability::Unknown);
        }
        self.calls += 1;
        // Charge verification reads even when the provider subsequently fails.
        self.bytes += object.bytes;
        match &mut self.reader {
            Reader::Existing(reader) => {
                let Source::Existing { profile } = &self.profile.source else {
                    unreachable!()
                };
                // An offline root is not a proved cache miss.
                if profile.places.iter().any(|p| !p.root.is_dir()) {
                    return Err(Availability::Unknown);
                }
                reader
                    .open_verified(&object)
                    .map(|r| Box::new(r) as Box<dyn Read>)
                    .map_err(|e| {
                        if e == tig_store::existing::Error::Missing {
                            Availability::Missing
                        } else {
                            Availability::Unknown
                        }
                    })
            }
            Reader::Native(root) => tig_store::local::LocalStore::open(root)
                .and_then(|s| s.read_existing_object(&object))
                .map(|r| Box::new(r) as Box<dyn Read>)
                .map_err(|_| Availability::Unknown),
        }
    }
    pub fn check(&mut self, key: &Key) -> Availability {
        match self.open(key) {
            Ok(_) => Availability::Present,
            Err(state) => state,
        }
    }
    pub fn retrieve(&mut self, key: &Key, destination: &Path) -> io::Result<()> {
        // Annex may pass a repository-relative staging filename. Resolve it
        // against the helper's cwd, then apply the same enrolled absolute scope.
        // Parent traversal is refused even if it would normalize inside scope.
        if destination
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(failure());
        }
        let destination = if destination.is_absolute() {
            destination.to_owned()
        } else {
            std::env::current_dir()?.join(destination)
        };
        let destination = destination.as_path();
        if !absolute(destination)
            || !self
                .profile
                .staging
                .iter()
                .any(|p| destination.starts_with(p) && destination != p)
        {
            return Err(failure());
        }
        let parent = destination.parent().ok_or_else(failure)?;
        let _directories = hold_directories(parent)?;
        let mut reader = self.open(key).map_err(|_| failure())?;
        // Refuse an old/linked/symlink destination. Annex may retry using a fresh
        // staging file; no interrupted file is ever silently overwritten.
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let start = Instant::now();
        let mut hash = Sha256::new();
        let mut count = 0u64;
        let mut buf = [0u8; 64 * 1024];
        loop {
            if start.elapsed() > Duration::from_millis(self.profile.transfer_millis) {
                return Err(failure());
            }
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            count = count.checked_add(n as u64).ok_or_else(failure)?;
            if count > key.object().bytes {
                return Err(failure());
            }
            hash.update(&buf[..n]);
            out.write_all(&buf[..n])?;
        }
        let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if count != key.object().bytes || digest != key.object().sha256.as_str() {
            return Err(failure());
        }
        out.sync_all()?;
        Ok(())
    }
}
fn failure() -> io::Error {
    io::Error::other("legacy cache request unavailable")
}
fn absolute(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
}
/// Hold ancestor directories against rename/reparse replacement on Windows.
pub(crate) fn hold_directories(path: &Path) -> io::Result<Vec<File>> {
    hold_directories_with_share(path, 1)
}
/// Hard-link creation needs directory metadata writes, while ancestor rename
/// and deletion remain excluded. Existing retrieval keeps its stricter mode.
pub(crate) fn hold_link_directories(path: &Path) -> io::Result<Vec<File>> {
    hold_directories_with_share(path, 3)
}
fn hold_directories_with_share(path: &Path, share: u32) -> io::Result<Vec<File>> {
    #[cfg(not(windows))]
    let _ = share;
    if !cfg!(windows) {
        return Err(io::Error::from(io::ErrorKind::Unsupported));
    }
    let mut held = Vec::new();
    for directory in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(share).custom_flags(0x02200000); // backup + open reparse point
        }
        let file = options.open(directory)?;
        let meta = file.metadata()?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(failure());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(failure());
            }
        }
        held.push(file);
    }
    Ok(held)
}
fn line(input: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let n = input.take(8193).read_until(b'\n', &mut bytes)?;
    if n == 0 {
        return Ok(None);
    }
    if n > 8192 || bytes.last() != Some(&b'\n') || bytes.contains(&0) {
        return Err(failure());
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes).map(Some).map_err(|_| failure())
}
pub fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    backend: &mut Backend,
) -> io::Result<()> {
    writeln!(output, "VERSION 2")?;
    output.flush()?;
    // Bound setup/optional chatter as well as payload requests.
    for _ in 0..4096 {
        let Some(request) = line(&mut input)? else {
            return Ok(());
        };
        let reply = match request.as_str() {
            "INITREMOTE" => "INITREMOTE-SUCCESS".into(),
            "PREPARE" => "PREPARE-SUCCESS".into(),
            "LISTCONFIGS" => "CONFIGEND".into(),
            "GETCOST" => "COST 100".into(),
            "GETAVAILABILITY" => "AVAILABILITY LOCAL".into(),
            "GETORDERED" => "ORDERED".into(),
            _ if request.starts_with("EXTENSIONS ") => "EXTENSIONS".into(),
            _ => respond(&request, backend),
        };
        writeln!(output, "{reply}")?;
        output.flush()?;
    }
    Err(failure())
}
fn respond(request: &str, backend: &mut Backend) -> String {
    let parts: Vec<_> = request.splitn(4, ' ').collect();
    match parts.as_slice() {
        ["CHECKPRESENT", raw] => match Key::parse(raw) {
            Ok(key) => match backend.check(&key) {
                Availability::Present => format!("CHECKPRESENT-SUCCESS {raw}"),
                Availability::Missing => format!("CHECKPRESENT-FAILURE {raw}"),
                Availability::Unknown => format!("CHECKPRESENT-UNKNOWN {raw} unavailable"),
            },
            Err(_) => format!("CHECKPRESENT-UNKNOWN {raw} unsupported-key"),
        },
        ["REMOVE", raw] => format!("REMOVE-FAILURE {raw} read-only"),
        ["TRANSFER", direction, raw, destination] if matches!(*direction, "STORE" | "RETRIEVE") => {
            let ok = *direction == "RETRIEVE"
                && Key::parse(raw)
                    .ok()
                    .is_some_and(|key| backend.retrieve(&key, Path::new(destination)).is_ok());
            if ok {
                format!("TRANSFER-SUCCESS {direction} {raw}")
            } else {
                format!("TRANSFER-FAILURE {direction} {raw} unavailable")
            }
        }
        _ => "UNSUPPORTED-REQUEST".into(),
    }
}

#[cfg(all(test, windows))]
mod link_namespace_tests {
    use super::*;

    #[test]
    fn link_guards_allow_link_metadata_but_fence_namespace_replacement() {
        let t = tempfile::tempdir().unwrap();
        let held = t.path().join("held");
        std::fs::create_dir(&held).unwrap();
        let source = held.join("source");
        std::fs::write(&source, b"synthetic").unwrap();
        let guards = hold_link_directories(&held).unwrap();
        std::fs::hard_link(&source, held.join("alias")).unwrap();
        assert!(same_file::is_same_file(&source, held.join("alias")).unwrap());
        assert!(std::fs::rename(&held, t.path().join("replacement")).is_err());
        let other = t.path().join("unrelated");
        std::fs::create_dir(&other).unwrap();
        std::fs::rename(&other, t.path().join("unrelated-moved")).unwrap();
        drop(guards);
        std::fs::rename(&held, t.path().join("replacement")).unwrap();
    }
}
