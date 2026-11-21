//! One metadata-read budget across every selected actor/path, including failures.
use std::cell::Cell;
use tig_core::{
    Object,
    tree::{NodeStore, TreeError, TreeLimits, TreeResult},
};

pub struct ReadScope<'a, S> {
    inner: &'a S,
    calls: Cell<usize>,
    bytes: Cell<usize>,
    limits: TreeLimits,
}
impl<'a, S> ReadScope<'a, S> {
    pub fn new(inner: &'a S, limits: TreeLimits) -> Self {
        Self {
            inner,
            calls: Cell::new(0),
            bytes: Cell::new(0),
            limits,
        }
    }
}
impl<S: NodeStore> NodeStore for ReadScope<'_, S> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        let calls = self.calls.get().checked_add(1).ok_or(TreeError::Budget)?;
        self.calls.set(calls);
        // Reserve the requested amount before IO, including misses and errors.
        let requested = usize::try_from(object.bytes).map_err(|_| TreeError::Budget)?;
        let bytes = self
            .bytes
            .get()
            .checked_add(requested)
            .ok_or(TreeError::Budget)?;
        self.bytes.set(bytes);
        if calls > self.limits.max_reads || bytes > self.limits.max_read_bytes {
            return Err(TreeError::Budget);
        }
        let value = self.inner.get(object)?;
        if value
            .as_ref()
            .is_some_and(|bytes| Object::from_bytes(bytes) != *object)
        {
            return Err(TreeError::Integrity);
        }
        Ok(value)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Budget)
    }
}
