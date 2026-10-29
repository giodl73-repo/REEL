//! Bounded contention handling for the pinned metadata provider, never a cache
//! or render lock. Immutable node retries keep the same provider and admission.
use std::{
    cell::Cell,
    time::{Duration, Instant},
};
use tig_core::{
    Object,
    tree::{NodeStore, TreeError, TreeResult},
};
use tig_store::metadata;

pub(super) struct MetadataWait {
    spent: Cell<Duration>,
    pauses: Cell<u16>,
    limit: Duration,
}
impl Default for MetadataWait {
    fn default() -> Self {
        Self {
            spent: Cell::new(Duration::ZERO),
            pauses: Cell::new(0),
            limit: Duration::from_secs(2),
        }
    }
}
impl MetadataWait {
    // One cumulative wait allowance spans open, freeze, finish and readback.
    // Compilation, capacity refusals, IO, integrity and rollback never retry.
    pub(super) fn run<T>(
        &self,
        action: impl FnMut() -> metadata::Result<T>,
    ) -> metadata::Result<T> {
        self.retry(action, |error| *error == metadata::Error::Busy)
    }
    fn retry<T, E>(
        &self,
        mut action: impl FnMut() -> Result<T, E>,
        busy: impl Fn(&E) -> bool,
    ) -> Result<T, E> {
        loop {
            let result = action();
            if !result.as_ref().is_err_and(&busy) {
                return result;
            }
            let remaining = self.limit.saturating_sub(self.spent.get());
            if remaining.is_zero() || self.pauses.get() >= 256 {
                return result;
            }
            self.pauses.set(self.pauses.get() + 1);
            let started = Instant::now();
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
            self.spent
                .set(self.spent.get().saturating_add(started.elapsed()));
        }
    }
}
pub(super) struct Waiting<'a, T> {
    pub(super) inner: T,
    wait: &'a MetadataWait,
}
impl<'a, T> Waiting<'a, T> {
    pub(super) fn new(inner: T, wait: &'a MetadataWait) -> Self {
        Self { inner, wait }
    }
}
// This exact private sentinel is the pinned tig-store metadata provider's
// NodeStore projection of Error::Busy. Do not classify arbitrary Store errors or
// other providers as retryable; all remaining errors return unchanged.
fn tree_busy(error: &TreeError) -> bool {
    matches!(error,TreeError::Store(message) if message == "metadata busy")
}
impl<T: NodeStore> NodeStore for Waiting<'_, T> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.wait.retry(|| self.inner.get(object), tree_busy)
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        self.wait.retry(|| self.inner.put(bytes), tree_busy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tig_store::metadata::{Limits, MetadataReader, MetadataStore, Stage};
    fn stores() -> (
        tempfile::TempDir,
        metadata::Checkpoint,
        MetadataStore,
        MetadataStore,
    ) {
        let root = tempfile::tempdir().unwrap();
        let mut first = MetadataStore::create(
            &root.path().join("nodes"),
            Limits {
                nodes: 64,
                bytes: 2 * 1024 * 1024,
                database_bytes: 4 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let minimum = first.finish().unwrap();
        let second = MetadataStore::open(&root.path().join("nodes"), &minimum).unwrap();
        (root, minimum, first, second)
    }
    fn hold(
        mut writer: MetadataStore,
    ) -> (
        std::sync::mpsc::Sender<()>,
        std::thread::JoinHandle<metadata::Checkpoint>,
        Object,
    ) {
        let object = writer.put(b"first concurrent writer").unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            writer
                .finish_hook(|stage| {
                    if stage == Stage::BeforeCommit {
                        ready_tx.send(()).unwrap();
                        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    }
                    Ok(())
                })
                .unwrap()
        });
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        (release_tx, thread, object)
    }
    #[test]
    fn metadata_writer_contention_retries_without_losing_either_immutable_object() {
        let (root, minimum, first, mut second) = stores();
        let other = second.put(b"second concurrent writer").unwrap();
        let (release, writer, one) = hold(first);
        let wait = MetadataWait::default();
        let mut release = Some(release);
        let mut calls = 0;
        let witness = wait
            .run(|| {
                calls += 1;
                let result = second.finish();
                if result.as_ref().is_err_and(|e| *e == metadata::Error::Busy) {
                    if let Some(release) = release.take() {
                        release.send(()).unwrap();
                    }
                }
                result
            })
            .unwrap();
        writer.join().unwrap();
        assert!(calls > 1);
        assert!(wait.pauses.get() > 0);
        drop(second);
        let reader = MetadataReader::open(&root.path().join("nodes"), &witness).unwrap();
        assert!(reader.checkpoint().generation() > minimum.generation());
        assert_eq!(
            reader.get(&one).unwrap().unwrap(),
            b"first concurrent writer"
        );
        assert_eq!(
            reader.get(&other).unwrap().unwrap(),
            b"second concurrent writer"
        );
    }
    #[test]
    fn exhausted_metadata_wait_keeps_pending_nodes_and_does_not_reset_for_next_operation() {
        let (root, _, first, mut second) = stores();
        let other = second.put(b"pending bytes survive refusal").unwrap();
        let (release, writer, one) = hold(first);
        let wait = MetadataWait {
            limit: Duration::from_millis(20),
            ..Default::default()
        };
        assert_eq!(
            wait.run(|| second.finish()).unwrap_err(),
            metadata::Error::Busy
        );
        let pauses = wait.pauses.get();
        let mut calls = 0;
        assert_eq!(
            wait.run(|| {
                calls += 1;
                second.finish()
            })
            .unwrap_err(),
            metadata::Error::Busy
        );
        assert_eq!(calls, 1);
        assert_eq!(wait.pauses.get(), pauses);
        release.send(()).unwrap();
        writer.join().unwrap();
        let witness = second.finish().unwrap();
        drop(second);
        let reader = MetadataReader::open(&root.path().join("nodes"), &witness).unwrap();
        assert!(reader.get(&one).unwrap().is_some());
        assert_eq!(
            reader.get(&other).unwrap().unwrap(),
            b"pending bytes survive refusal"
        );
    }
    #[test]
    fn foreign_checkpoint_is_terminal_without_spending_a_retry_budget() {
        let (root, minimum, first, second) = stores();
        drop(first);
        drop(second);
        let mut foreign = serde_json::to_value(minimum).unwrap();
        foreign["identity"] = serde_json::json!(tig_core::Digest::blob(b"foreign provider"));
        let minimum: metadata::Checkpoint = serde_json::from_value(foreign).unwrap();
        let wait = MetadataWait::default();
        let mut calls = 0;
        let result = wait.run(|| {
            calls += 1;
            MetadataReader::open(&root.path().join("nodes"), &minimum)
        });
        assert!(matches!(result, Err(metadata::Error::Foreign)));
        assert_eq!(calls, 1);
        assert_eq!(wait.spent.get(), Duration::ZERO);
        assert_eq!(wait.pauses.get(), 0);
    }

    #[test]
    fn buffered_node_flush_retries_the_actual_metadata_busy_projection() {
        let (root, _, first, mut second) = stores();
        for byte in 0_u8..16 {
            second.put(&vec![byte; 65536]).unwrap();
        }
        let (release, writer, first_object) = hold(first);
        struct Observed {
            inner: MetadataStore,
            release: Option<std::sync::mpsc::Sender<()>>,
            calls: usize,
        }
        impl NodeStore for Observed {
            fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
                self.inner.get(object)
            }
            fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
                self.calls += 1;
                let result = self.inner.put(bytes);
                if result.as_ref().is_err_and(tree_busy) {
                    if let Some(release) = self.release.take() {
                        release.send(()).unwrap();
                    }
                }
                result
            }
        }
        let wait = MetadataWait::default();
        let mut pending = Waiting::new(
            Observed {
                inner: second,
                release: Some(release),
                calls: 0,
            },
            &wait,
        );
        let body = vec![255; 65536];
        let object = pending.put(&body).unwrap();
        assert!(pending.inner.calls > 1);
        let witness = wait.run(|| pending.inner.inner.finish()).unwrap();
        writer.join().unwrap();
        drop(pending);
        let reader = MetadataReader::open(&root.path().join("nodes"), &witness).unwrap();
        assert!(reader.get(&first_object).unwrap().is_some());
        assert_eq!(reader.get(&object).unwrap().unwrap(), body);
        for byte in 0_u8..16 {
            let body = vec![byte; 65536];
            assert_eq!(
                reader.get(&Object::from_bytes(&body)).unwrap().unwrap(),
                body
            );
        }
    }
}
