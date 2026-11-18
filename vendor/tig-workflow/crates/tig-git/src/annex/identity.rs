//! Exact SHA256 annex identities; Git blobs are parsed, never followed as paths.
use tig_core::{Digest, Object};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
}

impl Key {
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value.len() > 100 {
            return Err(Error::Invalid);
        }
        let rest = value.strip_prefix("SHA256-s").ok_or(Error::Unsupported)?;
        let (size, hash) = rest.split_once("--").ok_or(Error::Invalid)?;
        let bytes = size.parse::<u64>().map_err(|_| Error::Invalid)?;
        if bytes.to_string() != size || Digest::parse(hash).is_err() {
            return Err(Error::Invalid);
        }
        Ok(Self(value.into()))
    }
    pub fn from_object(object: &Object) -> Self {
        Self(format!(
            "SHA256-s{}--{}",
            object.bytes,
            object.sha256.as_str()
        ))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn object(&self) -> Object {
        // Only the canonical parser and from_object can construct a key.
        let (size, hash) = self.0[8..].split_once("--").expect("canonical key");
        Object {
            sha256: Digest::parse(hash).expect("canonical digest"),
            bytes: size.parse().expect("canonical size"),
        }
    }
    pub fn unlocked_pointer(&self) -> Vec<u8> {
        format!("/annex/objects/{}\n", self.0).into_bytes()
    }
    /// Only the admitted unlocked and locked representations, not arbitrary
    /// substrings containing a key. Locked targets are inspected as Git data.
    pub fn pointer(bytes: &[u8], symlink: bool) -> Result<Self, Error> {
        if bytes.len() > 1024 {
            return Err(Error::Invalid);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| Error::Invalid)?;
        if !symlink {
            let key = text
                .strip_prefix("/annex/objects/")
                .and_then(|s| s.strip_suffix('\n'))
                .ok_or(Error::Invalid)?;
            return Self::parse(key);
        }
        let mut rest = text;
        while let Some(next) = rest.strip_prefix("../") {
            rest = next;
        }
        let tail = rest
            .strip_prefix(".git/annex/objects/")
            .ok_or(Error::Invalid)?;
        let parts: Vec<_> = tail.split('/').collect();
        if parts.len() != 4
            || parts[2] != parts[3]
            || !parts[..2]
                .iter()
                .all(|s| matches!(s.len(), 2 | 3) && s.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            return Err(Error::Invalid);
        }
        Self::parse(parts[3])
    }
}
