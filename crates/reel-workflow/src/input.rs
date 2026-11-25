//! Named immutable input access for storage adapters. No media semantics,
//! admission, retention, provider execution or latest-selection fallback.
use crate::{Artifact, Result};
/// Resolve only the exact declared Artifact and verify its original hash/length.
/// A native adapter must retain its actual verified byte/source guards for the
/// enclosing operation. Matching returned bytes alone prove no durable custody.
/// The caller supplies a per-read bound; adapters also enforce aggregate budgets.
pub trait InputReader {
    fn read(&mut self, artifact: &Artifact, max_bytes: u64) -> Result<Vec<u8>>;
}
