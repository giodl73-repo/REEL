//! Explicit declared host/tool byte identity, not inferred complete environment.
use crate::scene_delivery::{FileRef, InputFiles, MediaProgram};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

const COMPONENT_MAX: u64 = 1024 * 1024 * 1024;
const TOTAL_MAX: u64 = 4 * COMPONENT_MAX;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByteIdentity {
    pub sha256: String,
    pub bytes: u64,
}
impl ByteIdentity {
    /// Bounded read-only observation for preparing explicit local enrollment.
    /// This is not a protected capability; bind rechecks under effective guards.
    pub fn observe_file(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > COMPONENT_MAX {
            bail!("runtime observation exceeds budget/type");
        }
        let identity = read_identity(&mut file, metadata.len())?;
        if identity.bytes != metadata.len() || file.metadata()?.len() != metadata.len() {
            bail!("runtime observation size changed");
        }
        Ok(identity)
    }
    fn validate(&self) -> Result<()> {
        if self.bytes > COMPONENT_MAX
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            bail!("invalid bounded runtime identity");
        }
        Ok(())
    }
    fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            sha256: Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            bytes: bytes.len() as u64,
        }
    }
}

/// Private local enrollment; paths are never part of the portable manifest.
#[derive(Clone)]
pub struct Bindings {
    pub host: ByteIdentity,
    pub ffmpeg: FileRef,
    pub ffprobe: FileRef,
    pub dependencies: BTreeMap<String, FileRef>,
}
#[derive(Serialize)]
struct Manifest {
    schema: &'static str,
    host: ByteIdentity,
    programs: BTreeMap<&'static str, ByteIdentity>,
    dependencies: BTreeMap<String, ByteIdentity>,
    complete_environment: bool,
}
fn pin(file: &FileRef) -> ByteIdentity {
    ByteIdentity {
        sha256: file.sha256.clone(),
        bytes: file.bytes,
    }
}
fn dependency_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
        && name
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
fn program_path(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
}

struct HeldFile {
    _file: File,
    _directories: Vec<File>,
}
#[cfg(windows)]
fn hold_parents(path: &Path) -> Result<Vec<File>> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    let mut held = Vec::new();
    // Keep each canonical path component from being replaced/renamed. Child
    // additions and writes remain allowed; this is not a directory-wide write lock.
    for parent in path
        .ancestors()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let file = OpenOptions::new()
            .access_mode(0x80)
            .share_mode(3)
            .custom_flags(0x0220_0000)
            .open(parent)?;
        if !file.metadata()?.is_dir() || file.metadata()?.file_attributes() & 0x400 != 0 {
            bail!("runtime parent is not a protected ordinary directory");
        }
        held.push(file);
    }
    Ok(held)
}
fn hold(path: &Path, expected: &ByteIdentity) -> Result<(PathBuf, HeldFile)> {
    let path = path.canonicalize()?;
    if path.components().count() > 64 {
        bail!("runtime path exceeds component budget");
    }
    #[cfg(windows)]
    let directories = hold_parents(&path)?;
    #[cfg(not(windows))]
    let directories = Vec::new();
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ: refuse writers, replacement and delete while borrowed.
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let mut file = options.open(&path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if file.metadata()?.file_attributes() & 0x400 != 0 {
            bail!("runtime file is a reparse point");
        }
    }
    if !file.metadata()?.is_file() || file.metadata()?.len() != expected.bytes {
        bail!("declared runtime file size/type differs");
    }
    if &read_identity(&mut file, expected.bytes)? != expected
        || file.metadata()?.len() != expected.bytes
    {
        bail!("declared runtime file identity differs");
    }
    Ok((
        path,
        HeldFile {
            _file: file,
            _directories: directories,
        },
    ))
}
fn read_identity(file: &mut File, limit: u64) -> Result<ByteIdentity> {
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65_536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes = bytes
            .checked_add(n as u64)
            .ok_or_else(|| anyhow::anyhow!("runtime byte overflow"))?;
        if bytes > limit {
            bail!("declared runtime file grew");
        }
        hash.update(&buffer[..n]);
    }
    Ok(ByteIdentity {
        bytes,
        sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
    })
}

/// Held capability, never deserialized from a claimed success envelope.
/// Windows protection only; no cooperative fallback is advertised as mandatory.
pub struct BoundPrograms {
    _held: Vec<HeldFile>,
    programs: BTreeMap<MediaProgram, PathBuf>,
    manifest: Vec<u8>,
    identity: ByteIdentity,
}
impl BoundPrograms {
    pub fn bind(bindings: &Bindings) -> Result<Self> {
        bindings.host.validate()?;
        if bindings.dependencies.len() > 32
            || bindings.dependencies.keys().any(|n| !dependency_name(n))
            || !program_path(&bindings.ffmpeg.path)
            || !program_path(&bindings.ffprobe.path)
        {
            bail!("invalid declared runtime enrollment");
        }
        let mut total = bindings.host.bytes;
        for file in [&bindings.ffmpeg, &bindings.ffprobe]
            .into_iter()
            .chain(bindings.dependencies.values())
        {
            pin(file).validate()?;
            if !file.path.is_absolute() {
                bail!("runtime file requires an absolute path");
            }
            total = total
                .checked_add(file.bytes)
                .ok_or_else(|| anyhow::anyhow!("runtime budget overflow"))?;
        }
        if total > TOTAL_MAX {
            bail!("declared runtime exceeds byte budget");
        }
        if !cfg!(windows) {
            bail!("mandatory runtime path protection unavailable");
        }
        let (_, host) = hold(&std::env::current_exe()?, &bindings.host)?;
        let (ffmpeg, encoder) = hold(&bindings.ffmpeg.path, &pin(&bindings.ffmpeg))?;
        let (ffprobe, probe) = hold(&bindings.ffprobe.path, &pin(&bindings.ffprobe))?;
        if !program_path(&ffmpeg) || !program_path(&ffprobe) {
            bail!("shell batch programs are not native runtime bindings");
        }
        let mut held = vec![host, encoder, probe];
        for file in bindings.dependencies.values() {
            held.push(hold(&file.path, &pin(file))?.1);
        }
        let manifest = serde_json::to_vec(&Manifest {
            schema: "reel.declared-media-runtime.experimental-1",
            host: bindings.host.clone(),
            programs: BTreeMap::from([
                ("ffmpeg", pin(&bindings.ffmpeg)),
                ("ffprobe", pin(&bindings.ffprobe)),
            ]),
            dependencies: bindings
                .dependencies
                .iter()
                .map(|(name, file)| (name.clone(), pin(file)))
                .collect(),
            complete_environment: false,
        })?;
        if manifest.len() > 65_536 {
            bail!("runtime manifest exceeds budget");
        }
        let identity = ByteIdentity::from_bytes(&manifest);
        Ok(Self {
            _held: held,
            programs: BTreeMap::from([
                (MediaProgram::Ffmpeg, ffmpeg),
                (MediaProgram::Ffprobe, ffprobe),
            ]),
            manifest,
            identity,
        })
    }
    pub fn manifest_bytes(&self) -> &[u8] {
        &self.manifest
    }
    pub fn identity(&self) -> &ByteIdentity {
        &self.identity
    }
    pub fn with_inputs<'a, T: InputFiles + ?Sized>(
        &'a self,
        inputs: &'a T,
    ) -> RuntimeInputs<'a, T> {
        RuntimeInputs {
            inputs,
            programs: self,
        }
    }
}
pub struct RuntimeInputs<'a, T: ?Sized> {
    inputs: &'a T,
    programs: &'a BoundPrograms,
}
impl<T: InputFiles + ?Sized> InputFiles for RuntimeInputs<'_, T> {
    fn verified_path(&self, item: &FileRef) -> Result<PathBuf> {
        self.inputs.verified_path(item)
    }
    fn media_program(&self, program: MediaProgram) -> Result<PathBuf> {
        self.programs
            .programs
            .get(&program)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unbound media program"))
    }
}
