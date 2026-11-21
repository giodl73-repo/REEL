//! Full admitted pool roster plus compatible namespace-use and exact byte guards.
//! Participating pruners must use the same exclusive fences; no removal API.
use super::{Profile as AnnexProfile, absolute, line, process};
use git_asset_core::{Digest, Object};
use git_asset_store::{
    existing::{self, VerifiedRead},
    source_claims::{
        Checkpoint,
        admission::{self, Namespace, SubjectUse},
    },
    store_protection::Binding,
};
use serde::Deserialize;
use std::{io, path::PathBuf};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub root: PathBuf,
    pub minimum: Checkpoint,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub pool: AnnexProfile,
    pub binding: Binding,
    pub members: Vec<Member>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidRoster,
    WrongBinding,
    Annex(super::Error),
    Admission(admission::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;
impl From<super::Error> for Error {
    fn from(e: super::Error) -> Self {
        Self::Annex(e)
    }
}
impl From<admission::Error> for Error {
    fn from(e: admission::Error) -> Self {
        Self::Admission(e)
    }
}

pub struct Reader {
    reader: super::Reader,
    profile: Profile,
    binding: Digest,
}
impl Reader {
    /// Pure admission only; actual pool binding and namespace ancestry checked on open.
    pub fn new(mut profile: Profile) -> Result<Self> {
        let binding = profile.binding.id().map_err(|_| Error::InvalidRoster)?;
        if profile.members.len() != profile.binding.members().len()
            || profile.members.iter().any(|m| !absolute(&m.root))
        {
            return Err(Error::InvalidRoster);
        }
        profile
            .members
            .sort_by(|a, b| a.minimum.identity().cmp(b.minimum.identity()));
        if profile
            .members
            .iter()
            .zip(profile.binding.members())
            .any(|(location, member)| location.minimum.identity() != &member.namespace)
        {
            return Err(Error::InvalidRoster);
        }
        let reader = super::Reader::new(profile.pool.clone())?;
        Ok(Self {
            reader,
            profile,
            binding,
        })
    }
    pub fn store(&self) -> Digest {
        self.reader.store()
    }
    pub fn binding(&self) -> &Digest {
        &self.binding
    }
    pub fn attempts(&self) -> u32 {
        self.reader.attempts()
    }
    pub fn verification_bytes(&self) -> u64 {
        self.reader.verification_bytes()
    }
    pub fn open(&mut self, object: &Object) -> Result<Read> {
        self.reader.preflight(object)?;
        let _configuration = self.reader.admit_repository()?;
        let output = process::run(
            &self.profile.pool,
            false,
            &[
                "config",
                "--local",
                "--no-includes",
                "--get-all",
                "tig.protectionbinding",
            ],
        )?;
        if line(&output.stdout)? != self.binding.as_str() {
            return Err(Error::WrongBinding);
        }
        let mut held = Vec::with_capacity(self.profile.members.len());
        for (location, member) in self
            .profile
            .members
            .iter()
            .zip(self.profile.binding.members())
        {
            let namespace = Namespace::open(&location.root, &location.minimum)?;
            if namespace.identity() != &member.namespace {
                return Err(Error::InvalidRoster);
            }
            if !member.places.iter().all(|place| {
                namespace.policy().classes.values().any(|p| {
                    p.current
                        .iter()
                        .chain(&p.pending)
                        .chain(&p.predecessor)
                        .any(|key| key == place)
                })
            }) {
                return Err(Error::InvalidRoster);
            }
            held.push(namespace.hold(object)?);
        }
        let read = self.reader.open_local(object)?;
        Ok(Read { read, uses: held })
    }
}
/// The exact byte guard and every enrolled namespace use live together.
impl existing::ByteSource for Reader {
    type Reader = Read;
    fn open_verified(&mut self, object: &Object) -> existing::Result<Read> {
        self.open(object).map_err(|error| match error {
            Error::Annex(e) => super::byte_error(e),
            Error::InvalidRoster => existing::Error::Invalid,
            Error::WrongBinding => existing::Error::Denied,
            Error::Admission(
                admission::Error::Busy
                | admission::Error::Journal(git_asset_store::source_claims::Error::Busy),
            ) => existing::Error::Busy,
            Error::Admission(
                admission::Error::Budget
                | admission::Error::Journal(git_asset_store::source_claims::Error::Budget),
            ) => existing::Error::Budget,
            // Unknown enrollment/metadata is never payload absence.
            Error::Admission(_) => existing::Error::Io,
        })
    }
}
/// Keeps read and use guards alive together; not a durable job claim.
pub struct Read {
    read: VerifiedRead,
    uses: Vec<SubjectUse>,
}
impl Read {
    pub fn object(&self) -> &Object {
        self.read.object()
    }
    pub fn with_path<T>(
        &self,
        use_path: impl FnOnce(&std::path::Path) -> T,
    ) -> existing::Result<T> {
        self.read.with_path(use_path)
    }
    pub fn held_namespaces(&self) -> usize {
        self.uses.len()
    }
}
impl io::Read for Read {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        io::Read::read(&mut self.read, bytes)
    }
}
impl io::Seek for Read {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        io::Seek::seek(&mut self.read, position)
    }
}
