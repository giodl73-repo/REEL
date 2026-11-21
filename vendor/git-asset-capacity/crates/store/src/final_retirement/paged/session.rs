//! One caller-owned allowance across staging and independent metadata views.
use super::{Limits, Result};
use crate::protection::budget::{Budget, Nodes};
use git_asset_core::tree::NodeStore;

/// Clones share counters and the original deadline. This is resource admission,
/// not source authority, provider permission or a durable execution record.
#[derive(Clone)]
pub struct Session {
    limits: Limits,
    budget: Budget,
}
impl Session {
    pub fn new(limits: &Limits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            limits: limits.clone(),
            budget: Budget::new(&limits.source, limits.writes, limits.write_bytes)?,
        })
    }
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    pub fn check(&self) -> Result<()> {
        Ok(self.budget.timed()?)
    }
    /// Replace a writer with a reader without resetting spent or failed IO.
    pub fn nodes<'a, S: NodeStore>(&'a self, inner: &'a mut S) -> impl NodeStore + 'a {
        Nodes {
            inner,
            budget: Some(&self.budget),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_asset_core::{
        Object,
        tree::{MemoryNodes, TreeError},
    };

    #[test]
    fn cloned_session_preserves_failed_reads_and_writes_across_views() {
        let mut limits = Limits::default();
        limits.source.reads = 1;
        limits.writes = 1;
        let session = Session::new(&limits).unwrap();
        let mut first = MemoryNodes::default();
        let object = session.nodes(&mut first).put(b"original").unwrap();
        assert!(
            session
                .nodes(&mut first)
                .get(&Object::from_bytes(b"missing"))
                .unwrap()
                .is_none()
        );
        let same = session.clone();
        let mut other = MemoryNodes::default();
        assert!(matches!(
            same.nodes(&mut other).get(&object),
            Err(TreeError::Budget)
        ));
        assert!(matches!(
            same.nodes(&mut other).put(b"second"),
            Err(TreeError::Budget)
        ));
        assert!(other.get(&Object::from_bytes(b"second")).unwrap().is_none());
    }
}
