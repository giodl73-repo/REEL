//! Explicit local annex reads. No fetching, initialization, publication or removal.
use git_asset_core::{Digest, Object};
use git_asset_store::existing::{
    self, ByteSource, Limits, ProviderPath, ReadCache, RequiredProtection, VerifiedRead,
};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
pub mod managed;
mod process;
#[cfg(test)]
mod tests;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub git: PathBuf,
    pub annex: PathBuf,
    pub repository: PathBuf,
    #[serde(default)]
    pub bare: bool,
    pub uuid: String,
    /// Exact .git/config (ordinary) or config (bare); at most 64 KiB.
    pub configuration: Object,
    pub tools: Vec<PathBuf>,
    pub empty_config: PathBuf,
    pub empty_hooks: PathBuf,
    pub temporary: PathBuf,
    pub command_millis: u64,
    pub output_bytes: usize,
    #[serde(default)]
    pub limits: Limits,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
    WrongIdentity,
    Observation,
    Missing,
    Budget,
    Configuration(existing::Error),
    Bytes(existing::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

pub struct Reader {
    profile: Profile,
    metadata: ReadCache,
    data: ReadCache,
}
impl Reader {
    /// Pure admission only; no process, filesystem discovery or initialization.
    pub fn new(profile: Profile) -> Result<Self> {
        let paths = [
            &profile.git,
            &profile.annex,
            &profile.repository,
            &profile.empty_config,
            &profile.empty_hooks,
            &profile.temporary,
        ];
        if paths.iter().any(|p| !absolute(p))
            || profile.tools.is_empty()
            || profile.tools.len() > 16
            || profile.tools.iter().any(|p| !absolute(p))
            || !uuid(&profile.uuid)
            || profile.configuration.bytes == 0
            || profile.configuration.bytes > 65536
            || !(1..=120000).contains(&profile.command_millis)
            || !(256..=16384).contains(&profile.output_bytes)
        {
            return Err(Error::Invalid);
        }
        let data = ReadCache::provider(RequiredProtection::PreventMutation, profile.limits)
            .map_err(Error::Bytes)?;
        let metadata = ReadCache::provider(
            RequiredProtection::PreventMutation,
            Limits {
                object_bytes: 65536,
                total_bytes: 65536 * u64::from(profile.limits.requests),
                requests: profile.limits.requests,
                live_readers: 1,
                verify_millis: profile.limits.verify_millis,
            },
        )
        .map_err(Error::Configuration)?;
        Ok(Self {
            profile,
            metadata,
            data,
        })
    }
    pub fn store(&self) -> Digest {
        Digest::blob(format!("tig.annex-store.design-1\n{}\n", self.profile.uuid).as_bytes())
    }
    pub fn attempts(&self) -> u32 {
        self.metadata.requests()
    }
    pub fn verification_bytes(&self) -> u64 {
        self.data.verification_bytes()
    }
    /// Synchronous Windows adapter; use a dedicated thread outside entered Tokio runtimes.
    pub fn open(&mut self, object: &Object) -> Result<VerifiedRead> {
        self.preflight(object)?;
        let _configuration = self.admit_repository()?;
        self.open_local(object)
    }
    fn preflight(&self, object: &Object) -> Result<()> {
        if !process::supported() {
            return Err(Error::Unsupported);
        }
        let l = self.profile.limits;
        if object.bytes > l.object_bytes
            || self
                .data
                .verification_bytes()
                .checked_add(object.bytes)
                .is_none_or(|n| n > l.total_bytes)
            || self.data.active_readers() >= l.live_readers
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
    fn admit_repository(&mut self) -> Result<VerifiedRead> {
        if !process::supported() {
            return Err(Error::Unsupported);
        }
        let config = ProviderPath::parse(if self.profile.bare {
            "config"
        } else {
            ".git/config"
        })
        .map_err(Error::Configuration)?;
        let config = self
            .metadata
            .open_provider_file(
                &self.profile.repository,
                &config,
                &self.profile.configuration,
            )
            .map_err(Error::Configuration)?;
        let prefix = if self.profile.bare {
            "annex/objects/"
        } else {
            ".git/annex/objects/"
        };
        // Refuse an unenrolled namespace before invoking annex; never initialize it.
        let mut namespace = self.profile.repository.clone();
        for part in prefix.trim_end_matches('/').split('/') {
            namespace.push(part);
            let m = std::fs::symlink_metadata(&namespace).map_err(|_| Error::Observation)?;
            if !m.is_dir() || reparse(&m) {
                return Err(Error::Unsupported);
            }
        }
        let observed = process::run(
            &self.profile,
            false,
            &[
                "config",
                "--local",
                "--no-includes",
                "--get-all",
                "annex.uuid",
            ],
        )?;
        let observed = line(&observed.stdout)?;
        if observed != self.profile.uuid || observed.is_empty() {
            return Err(Error::WrongIdentity);
        }
        if observed.len() != 36 {
            return Err(Error::WrongIdentity);
        }
        let version = process::run(
            &self.profile,
            false,
            &[
                "config",
                "--local",
                "--no-includes",
                "--get-all",
                "annex.version",
            ],
        )?;
        if line(&version.stdout)? != "10" {
            return Err(Error::Unsupported);
        }
        Ok(config)
    }
    fn open_local(&mut self, object: &Object) -> Result<VerifiedRead> {
        let prefix = if self.profile.bare {
            "annex/objects/"
        } else {
            ".git/annex/objects/"
        };
        let key = format!("SHA256-s{}--{}", object.bytes, object.sha256.as_str());
        let location = process::run(&self.profile, true, &["contentlocation", &key]);
        let location = match location {
            Err(Error::Missing) => return Err(Error::Missing),
            other => other?,
        };
        let path = location_path(&location.stdout, prefix, &key)?;
        // Caller keeps the configuration guard live while acquiring this data guard.
        // Config edits after return cannot mutate/replace the held payload namespace.
        self.data
            .open_provider_file(&self.profile.repository, &path, object)
            .map_err(Error::Bytes)
    }
}
impl ByteSource for Reader {
    type Reader = VerifiedRead;
    fn open_verified(&mut self, object: &Object) -> existing::Result<VerifiedRead> {
        self.open(object).map_err(byte_error)
    }
}
fn byte_error(error: Error) -> existing::Error {
    match error {
        Error::Invalid => existing::Error::Invalid,
        Error::Unsupported => existing::Error::Unsupported,
        Error::WrongIdentity => existing::Error::Denied,
        Error::Observation => existing::Error::Io,
        Error::Missing => existing::Error::Missing,
        Error::Budget => existing::Error::Budget,
        Error::Configuration(e) | Error::Bytes(e) => e,
    }
}
fn absolute(p: &Path) -> bool {
    p.is_absolute()
        && p.as_os_str().len() <= 4096
        && !p
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}
fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(n, b)| {
            if [8, 13, 18, 23].contains(&n) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
fn line(bytes: &[u8]) -> Result<&str> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| Error::Observation)?
        .trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.contains(['\r', '\n']) {
        return Err(Error::Observation);
    }
    Ok(value)
}
fn location_path(bytes: &[u8], prefix: &str, key: &str) -> Result<ProviderPath> {
    // Only provider-issued spelling is normalized; logical paths are unchanged.
    // Preserve empty/parent components so the provider parser rejects escapes.
    let native = line(bytes)?.replace('\\', "/");
    let value = native
        .split('/')
        .filter(|part| *part != ".")
        .collect::<Vec<_>>()
        .join("/");
    if !value.starts_with(prefix) || value.rsplit('/').next() != Some(key) {
        return Err(Error::Observation);
    }
    ProviderPath::parse(&value).map_err(|_| Error::Observation)
}
fn reparse(m: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        m.file_type().is_symlink()
    }
}
