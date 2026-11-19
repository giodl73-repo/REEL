//! Task-bound return allowances. Operational return paths cannot expand them.
use super::*;
use sha2::{Digest as _, Sha256};
use std::{
    io::Read,
    time::{Duration, Instant},
};
#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub schema: String,
    pub total_bytes: u64,
    pub object_bytes: u64,
    /// Per-file verification deadline, checked between blocking reads.
    pub verify_millis: u64,
}
impl Budget {
    pub fn validate(&self) -> Result<()> {
        if self.schema != "reel.workflow-return-budget.v1" {
            return Err(error(ErrorKind::Invalid, "response-budget-version"));
        }
        if self.object_bytes == 0
            || self.object_bytes > self.total_bytes
            || self.total_bytes > 16 * 1024 * 1024 * 1024
            || self.verify_millis == 0
            || self.verify_millis > 300_000
        {
            return Err(error(ErrorKind::Budget, "response-budget"));
        }
        Ok(())
    }
    pub(crate) fn check_task(&self, packet: &Packet) -> Result<()> {
        let mut total = if packet.task.tool.is_some() {
            reel_workflow::MAX_CONTROL_BYTES as u64
        } else {
            0
        };
        if total > self.object_bytes {
            return Err(error(
                ErrorKind::Budget,
                "response-worker-receipt-allowance",
            ));
        }
        for output in packet.task.outputs.values() {
            if output.max_bytes > self.object_bytes {
                return Err(error(ErrorKind::Budget, "response-declared-object-limit"));
            }
            total = total
                .checked_add(output.max_bytes)
                .filter(|n| *n <= self.total_bytes)
                .ok_or_else(|| error(ErrorKind::Budget, "response-declared-byte-limit"))?;
        }
        Ok(())
    }
}

pub(crate) struct Bound {
    pub(crate) total: u64,
    pub(crate) object: u64,
    deadline: Option<Duration>,
}
impl Bound {
    pub(crate) fn open(packet: &Packet, source: &NativeProfile) -> Result<Self> {
        source.snapshot.check_binding(packet)?;
        Ok(
            match source
                .snapshot
                .budget
                .as_ref()
                .and_then(|b| b.returns.as_ref())
            {
                Some(b) => Self {
                    total: b.total_bytes,
                    object: b.object_bytes,
                    deadline: Some(Duration::from_millis(b.verify_millis)),
                },
                None => Self {
                    total: MAX_BYTES,
                    object: MAX_BYTES,
                    deadline: None,
                },
            },
        )
    }
    pub(crate) fn check(&self, result: &ReturnedResult) -> Result<()> {
        if result.artifacts.len() > reel_publication::MAX_OUTPUTS {
            return Err(error(ErrorKind::Budget, "response-output-count"));
        }
        let mut total = 0u64;
        for artifact in artifacts(result) {
            total = total
                .checked_add(artifact.bytes)
                .filter(|n| *n <= self.total)
                .ok_or_else(|| error(ErrorKind::Budget, "response-byte-limit"))?;
            if artifact.bytes > self.object {
                return Err(error(ErrorKind::Budget, "response-object-limit"));
            }
        }
        Ok(())
    }
    pub(crate) fn verify(&self, artifact: &Artifact, reader: &mut impl Read) -> Result<()> {
        if artifact.bytes > self.object {
            return Err(error(ErrorKind::Budget, "response-object-limit"));
        }
        let started = Instant::now();
        let mut hash = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            self.check_elapsed(started)?;
            let count = match reader.read(&mut buffer) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(io_error(e)),
            };
            self.check_elapsed(started)?;
            if count == 0 {
                break;
            }
            total = total
                .checked_add(count as u64)
                .filter(|n| *n <= artifact.bytes)
                .ok_or_else(|| error(ErrorKind::Integrity, "output-content"))?;
            hash.update(&buffer[..count]);
        }
        let actual: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if total != artifact.bytes || actual != artifact.sha256 {
            return Err(error(ErrorKind::Integrity, "output-content"));
        }
        Ok(())
    }
    pub(super) fn check_elapsed(&self, started: Instant) -> Result<()> {
        if self.deadline.is_some_and(|d| started.elapsed() >= d) {
            Err(error(ErrorKind::Budget, "response-verification-deadline"))
        } else {
            Ok(())
        }
    }
}
