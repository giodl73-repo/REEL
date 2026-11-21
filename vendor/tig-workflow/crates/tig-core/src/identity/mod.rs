//! Shared strict identity primitives. The legacy semantic frame is intentionally
//! preserved for Art parity; raw blob SHA-256 is the Git LFS byte identity.
mod canonical;

pub use canonical::{Digest, canonical_json, parse_json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error(message.into()))
    }
}
