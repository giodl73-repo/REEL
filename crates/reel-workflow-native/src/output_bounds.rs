//! Internal producer admission ceilings. Never serialized or inferred from an
//! arbitrary returned result. Public external-provider bounds stay unchanged.
use crate::error;
use reel_workflow::{ErrorKind, Result, ReturnedResult};

#[derive(Clone, Copy)]
pub(crate) struct Allowance {
    payload: u64,
    total: u64,
    film: bool,
}
impl Allowance {
    pub fn admission_ceiling(task: &reel_workflow::Task) -> u64 {
        if task
            .tool
            .as_ref()
            .is_some_and(|tool| tool.capability == reel_workflow::ToolCapability::FilmRender)
        {
            crate::admission::limits().logical_bytes
        } else {
            Self::EXTERNAL.total
        }
    }
    pub const EXTERNAL: Self = Self {
        payload: 128 * 1024 * 1024,
        total: 128 * 1024 * 1024,
        film: false,
    };
    #[cfg(all(windows, feature = "film"))]
    pub fn film(payload: u64) -> Result<Self> {
        if payload == 0 || payload > 16 * 1024 * 1024 * 1024 {
            return Err(error(ErrorKind::Budget, "film-output-allowance"));
        }
        Ok(Self {
            payload,
            total: payload + tig_store::metadata::MAX_NODE_BYTES as u64,
            film: true,
        })
    }
    pub fn object_bytes(self) -> u64 {
        if self.film {
            self.payload.max(tig_store::metadata::MAX_NODE_BYTES as u64)
        } else {
            self.payload
        }
    }
    pub fn check(self, result: &ReturnedResult) -> Result<()> {
        let payload = result
            .artifacts
            .values()
            .try_fold(0u64, |n, a| n.checked_add(a.bytes));
        let control = result.worker_receipt.as_ref().map_or(0, |a| a.bytes);
        if result.artifacts.len() + usize::from(result.worker_receipt.is_some()) > 32
            || payload.is_none_or(|n| {
                n > self.payload || n.checked_add(control).is_none_or(|s| s > self.total)
            })
            || (self.film && control > tig_store::metadata::MAX_NODE_BYTES as u64)
        {
            return Err(error(ErrorKind::Budget, "native-output-allowance"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reel_workflow::{Artifact, Mode, Provenance};
    use std::collections::BTreeMap;
    fn artifact(bytes: u64) -> Artifact {
        Artifact {
            logical_id: "synthetic/output".into(),
            sha256: "a".repeat(64),
            bytes,
            media_type: "application/octet-stream".into(),
        }
    }
    fn result(bytes: u64) -> ReturnedResult {
        ReturnedResult {
            schema: reel_workflow::RESULT_SCHEMA.into(),
            task_id: "synthetic".into(),
            task_key: "b".repeat(64),
            attempt_id: "attempt-1".into(),
            artifacts: BTreeMap::from([("output".into(), artifact(bytes))]),
            provenance: Provenance {
                mode: Mode::Handoff,
                provider: None,
                model: None,
                executable: None,
            },
            worker_receipt: None,
        }
    }
    #[test]
    fn external_limit_stays_aggregate_including_control_and_count() {
        let mut r = result(128 * 1024 * 1024);
        assert!(Allowance::EXTERNAL.check(&r).is_ok());
        r.worker_receipt = Some(artifact(1));
        assert!(Allowance::EXTERNAL.check(&r).is_err());
        assert!(
            Allowance::EXTERNAL
                .check(&result(129 * 1024 * 1024))
                .is_err()
        );
        r.artifacts = (0..33).map(|n| (n.to_string(), artifact(1))).collect();
        assert!(Allowance::EXTERNAL.check(&r).is_err());
        r.artifacts = BTreeMap::from([
            ("first".into(), artifact(u64::MAX)),
            ("second".into(), artifact(1)),
        ]);
        assert!(Allowance::EXTERNAL.check(&r).is_err());
    }
    #[cfg(all(windows, feature = "film"))]
    #[test]
    fn film_allowance_is_explicit_and_cannot_borrow_worker_headroom_for_media() {
        let bytes = 3 * 1024 * 1024 * 1024;
        let allowance = Allowance::film(bytes).unwrap();
        let mut r = result(bytes);
        r.worker_receipt = Some(artifact(tig_store::metadata::MAX_NODE_BYTES as u64));
        assert!(allowance.check(&r).is_ok());
        assert_eq!(allowance.object_bytes(), bytes);
        r.artifacts.get_mut("output").unwrap().bytes += 1;
        r.worker_receipt = None;
        assert!(allowance.check(&r).is_err());
        r.artifacts.get_mut("output").unwrap().bytes = bytes;
        r.worker_receipt = Some(artifact(tig_store::metadata::MAX_NODE_BYTES as u64 + 1));
        assert!(allowance.check(&r).is_err());
        assert!(Allowance::film(0).is_err());
        assert!(Allowance::film(16 * 1024 * 1024 * 1024 + 1).is_err());
        assert!(Allowance::film(16 * 1024 * 1024 * 1024).is_ok());
    }
}
