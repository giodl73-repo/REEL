//! Read-only configured metadata headroom. This neither reserves future writes
//! nor estimates the node cost of a future lifecycle transition.
use crate::{admission, error};
use reel_workflow::{ErrorKind, Result};
use serde::Serialize;
use tig_store::metadata;

#[derive(Serialize)]
pub struct Capacity {
    pub checkpoint: metadata::Checkpoint,
    pub limits: metadata::Limits,
    pub remaining_nodes: u64,
    pub remaining_bytes: u64,
}
pub fn inspect(profile: &admission::Profile) -> Result<Capacity> {
    if profile.schema != "reel.workflow-native-admission.v1" {
        return Err(error(
            ErrorKind::Invalid,
            "metadata-capacity-profile-schema",
        ));
    }
    let reader = metadata::MetadataReader::open(&profile.metadata_root, &profile.metadata_minimum)
        .map_err(|e| {
            let (kind, code) = match e {
                metadata::Error::Foreign => (ErrorKind::Conflict, "metadata-capacity-foreign"),
                metadata::Error::Rollback => (ErrorKind::Stale, "metadata-capacity-rollback"),
                metadata::Error::Corrupt => (ErrorKind::Integrity, "metadata-capacity-corrupt"),
                metadata::Error::Budget => (ErrorKind::Budget, "metadata-capacity-budget"),
                metadata::Error::Busy => (ErrorKind::Busy, "metadata-capacity-busy"),
                _ => (ErrorKind::NeedsRecovery, "metadata-capacity-unavailable"),
            };
            error(kind, code)
        })?;
    let checkpoint = reader.checkpoint().clone();
    let limits = reader.limits();
    Ok(Capacity {
        remaining_nodes: limits.nodes.saturating_sub(checkpoint.nodes()),
        remaining_bytes: limits.bytes.saturating_sub(checkpoint.bytes()),
        checkpoint,
        limits,
    })
}
