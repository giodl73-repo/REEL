//! Call-owned metadata admission across protocol phases; no persistent engine.
use super::*;
use std::{ops::Deref, rc::Rc};

#[derive(Clone)]
pub(crate) struct Budget(Rc<Inner>);
pub(crate) struct Inner {
    limits: Limits,
    start: Instant,
    reads: Cell<usize>,
    read_bytes: Cell<usize>,
    writes: Cell<usize>,
    write_bytes: Cell<usize>,
    max_writes: usize,
    max_write_bytes: usize,
}
impl Deref for Budget {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}
impl Budget {
    #[cfg(test)]
    pub fn usage(&self) -> (usize, usize, usize, usize) {
        (
            self.reads.get(),
            self.read_bytes.get(),
            self.writes.get(),
            self.write_bytes.get(),
        )
    }
    pub fn new(limits: &Limits, writes: usize, write_bytes: usize) -> TreeResult<Self> {
        limits.validate().map_err(|_| TreeError::Budget)?;
        Ok(Self(Rc::new(Inner {
            limits: limits.clone(),
            start: Instant::now(),
            reads: Cell::new(0),
            read_bytes: Cell::new(0),
            writes: Cell::new(0),
            write_bytes: Cell::new(0),
            max_writes: writes,
            max_write_bytes: write_bytes,
        })))
    }
    pub fn timed(&self) -> TreeResult<()> {
        if self.start.elapsed() > Duration::from_millis(self.limits.millis) {
            Err(TreeError::Budget)
        } else {
            Ok(())
        }
    }
    pub(crate) fn read(&self, o: &Object) -> TreeResult<()> {
        self.timed()?;
        let reads = self.reads.get().checked_add(1).ok_or(TreeError::Budget)?;
        let bytes = self
            .read_bytes
            .get()
            .checked_add(usize::try_from(o.bytes).map_err(|_| TreeError::Budget)?)
            .ok_or(TreeError::Budget)?;
        if reads > self.limits.reads || bytes > self.limits.read_bytes {
            return Err(TreeError::Budget);
        }
        self.reads.set(reads);
        self.read_bytes.set(bytes);
        Ok(())
    }
    pub(crate) fn write(&self, raw: &[u8]) -> TreeResult<()> {
        self.timed()?;
        let writes = self.writes.get().checked_add(1).ok_or(TreeError::Budget)?;
        let bytes = self
            .write_bytes
            .get()
            .checked_add(raw.len())
            .ok_or(TreeError::Budget)?;
        if writes > self.max_writes || bytes > self.max_write_bytes {
            return Err(TreeError::Budget);
        }
        self.writes.set(writes);
        self.write_bytes.set(bytes);
        Ok(())
    }
}

/// Borrow another metadata view while retaining the same budget through finish,
/// independent readback, journal admission and final provider clearance.
pub(crate) struct Nodes<'a, S> {
    pub inner: &'a mut S,
    pub budget: Option<&'a Budget>,
}
impl<S: NodeStore> NodeStore for Nodes<'_, S> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if let Some(b) = self.budget {
            b.read(o)?;
        }
        let value = self.inner.get(o)?;
        if let Some(b) = self.budget {
            b.timed()?;
        }
        Ok(value)
    }
    fn put(&mut self, raw: &[u8]) -> TreeResult<Object> {
        if let Some(b) = self.budget {
            b.write(raw)?;
        }
        let object = self.inner.put(raw)?;
        if let Some(b) = self.budget {
            b.timed()?;
        }
        Ok(object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_asset_core::tree::MemoryNodes;
    #[test]
    fn replacing_views_cannot_reset_failed_read_or_write_allowance() {
        let mut first = MemoryNodes::default();
        let object = first.put(b"node").unwrap();
        let limits = Limits {
            millis: 60000,
            roots: 1,
            bindings: 16,
            associations: 16,
            reads: 2,
            read_bytes: 65536,
        };
        let budget = Budget::new(&limits, 0, 0).unwrap();
        assert!(
            Nodes {
                inner: &mut first,
                budget: Some(&budget)
            }
            .get(&Object::from_bytes(b"absent"))
            .unwrap()
            .is_none()
        );
        assert!(
            Nodes {
                inner: &mut first,
                budget: Some(&budget)
            }
            .get(&object)
            .unwrap()
            .is_some()
        );
        let mut second = MemoryNodes::default();
        assert!(matches!(
            Nodes {
                inner: &mut second,
                budget: Some(&budget)
            }
            .get(&object),
            Err(TreeError::Budget)
        ));
        assert!(matches!(
            Nodes {
                inner: &mut second,
                budget: Some(&budget)
            }
            .put(b"write"),
            Err(TreeError::Budget)
        ));
        assert_eq!(budget.usage().0, 2);
        let same_budget = budget.clone();
        assert!(matches!(same_budget.read(&object), Err(TreeError::Budget)));
        assert_eq!(same_budget.usage(), budget.usage());
        assert!(second.get(&Object::from_bytes(b"write")).unwrap().is_none());
    }
}
