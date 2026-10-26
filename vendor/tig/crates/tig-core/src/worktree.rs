//! Pure private bindings and immutable candidate/adoption plans, not a mount or
//! durable worktree. Payload pins here do not prove publication or custody.
use crate::{LogicalPath, Object, tree::*};
use std::collections::BTreeMap;

pub const MAX_CHANGES: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreeError {
    Tree(TreeError),
    OverlappingPaths,
    Budget,
    StaleTarget,
}
impl From<TreeError> for WorktreeError {
    fn from(value: TreeError) -> Self {
        Self::Tree(value)
    }
}
impl std::fmt::Display for WorktreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for WorktreeError {}
pub type WorktreeResult<T> = Result<T, WorktreeError>;

fn overlaps(a: &LogicalPath, b: &LogicalPath) -> bool {
    fn ancestor(a: &str, b: &str) -> bool {
        b.strip_prefix(a).is_some_and(|rest| rest.starts_with('/'))
    }
    ancestor(a.as_str(), b.as_str()) || ancestor(b.as_str(), a.as_str())
}

/// Frozen shared base plus exact-file private puts/tombstones. Cloning copies
/// bounded bindings only, never payloads or the base metadata closure.
#[derive(Clone, Debug)]
pub struct ByteWorktree {
    base: Tree,
    changes: BTreeMap<LogicalPath, Option<Object>>,
}
impl ByteWorktree {
    pub fn open(base: Tree) -> Self {
        Self {
            base,
            changes: BTreeMap::new(),
        }
    }
    pub fn base(&self) -> &Tree {
        &self.base
    }
    pub fn changes(&self) -> &BTreeMap<LogicalPath, Option<Object>> {
        &self.changes
    }

    /// Receives a completed object pin. Actual byte verification/publication and
    /// durable protection must precede real Git adoption in the provider layer.
    pub fn put(&mut self, path: LogicalPath, object: Object) -> WorktreeResult<()> {
        self.record(path, Some(object))
    }
    /// Exact leaf removal only. This cannot delete bytes or recursively hide a directory.
    pub fn remove(&mut self, path: LogicalPath) -> WorktreeResult<()> {
        self.record(path, None)
    }
    fn record(&mut self, path: LogicalPath, value: Option<Object>) -> WorktreeResult<()> {
        if !self.changes.contains_key(&path) && self.changes.len() >= MAX_CHANGES {
            return Err(WorktreeError::Budget);
        }
        if self.changes.keys().any(|p| overlaps(p, &path)) {
            return Err(WorktreeError::OverlappingPaths);
        }
        if crate::check_sizes(
            self.changes
                .iter()
                .filter(|(p, _)| *p != &path)
                .filter_map(|(_, object)| object.as_ref())
                .chain(value.iter()),
        )
        .is_err()
        {
            return Err(TreeError::Integrity.into());
        }
        self.changes.insert(path, value);
        Ok(())
    }
    /// Discards one private binding only; no provider cleanup is implied.
    pub fn discard(&mut self, path: &LogicalPath) -> bool {
        self.changes.remove(path).is_some()
    }
    pub fn get(
        &self,
        path: &LogicalPath,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> WorktreeResult<(Option<Object>, TreeMetrics)> {
        if let Some(value) = self.changes.get(path) {
            return Ok((value.clone(), TreeMetrics::default()));
        }
        if self.changes.keys().any(|p| overlaps(p, path)) {
            return Err(WorktreeError::OverlappingPaths);
        }
        Ok(self.base.get(path, store, limits)?)
    }
    /// Resolves exact before/after pins and constructs an immutable tree.
    /// Private state remains intact on success/failure; callers adopt explicitly.
    pub fn seal(
        &self,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> WorktreeResult<(SealedCandidate, TreeMetrics)> {
        let mut budget = Budget::new(limits);
        let mut changes = BTreeMap::new();
        for (path, after) in &self.changes {
            let before = budget.get(&self.base, path, store)?;
            if before != *after {
                changes.insert(
                    path.clone(),
                    BindingChange {
                        before,
                        after: after.clone(),
                    },
                );
            }
        }
        let edits = changes
            .iter()
            .map(|(p, c)| (p.clone(), c.after.clone()))
            .collect();
        let tree = budget.edit(&self.base, &edits, store)?;
        Ok((
            SealedCandidate {
                base: self.base.clone(),
                tree,
                changes,
            },
            budget.metrics(),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingChange {
    pub before: Option<Object>,
    pub after: Option<Object>,
}

#[derive(Clone, Debug)]
pub struct SealedCandidate {
    base: Tree,
    tree: Tree,
    changes: BTreeMap<LogicalPath, BindingChange>,
}
impl SealedCandidate {
    pub fn base(&self) -> &Tree {
        &self.base
    }
    pub fn tree(&self) -> &Tree {
        &self.tree
    }
    pub fn changes(&self) -> &BTreeMap<LogicalPath, BindingChange> {
        &self.changes
    }

    /// Three-way comparison on the touched leaves. Conflicts produce no writes.
    /// A successful plan is not approval, a Git edit, or a durable ref update.
    pub fn plan_adoption(
        &self,
        current: &Tree,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> WorktreeResult<(Adoption, TreeMetrics)> {
        let mut budget = Budget::new(limits);
        let mut conflicts = Vec::new();
        let mut edits = BTreeMap::new();
        for (path, change) in &self.changes {
            let now = match budget.get(current, path, store) {
                Ok(now) => now,
                Err(WorktreeError::Tree(TreeError::PathConflict)) => {
                    conflicts.push(BindingConflict {
                        path: path.clone(),
                        change: change.clone(),
                        current: None,
                        kind: ConflictKind::PathShape,
                    });
                    continue;
                }
                Err(error) => return Err(error),
            };
            if now == change.after {
                continue;
            }
            if now == change.before {
                edits.insert(path.clone(), change.after.clone());
            } else {
                conflicts.push(BindingConflict {
                    path: path.clone(),
                    change: change.clone(),
                    current: now,
                    kind: ConflictKind::Content,
                });
            }
        }
        let result = if conflicts.is_empty() {
            Adoption::Ready(AdoptionPlan {
                expected_root: current.root().clone(),
                edits,
            })
        } else {
            Adoption::Conflicts(conflicts)
        };
        Ok((result, budget.metrics()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    Content,
    PathShape,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingConflict {
    pub path: LogicalPath,
    pub change: BindingChange,
    /// None is an absent file only for Content; PathShape indicates a prefix/directory conflict.
    pub current: Option<Object>,
    pub kind: ConflictKind,
}
#[derive(Clone, Debug)]
pub enum Adoption {
    Ready(AdoptionPlan),
    Conflicts(Vec<BindingConflict>),
}
#[derive(Clone, Debug)]
pub struct AdoptionPlan {
    expected_root: Object,
    edits: BTreeMap<LogicalPath, Option<Object>>,
}
impl AdoptionPlan {
    pub fn expected_root(&self) -> &Object {
        &self.expected_root
    }
    pub fn edits(&self) -> &BTreeMap<LogicalPath, Option<Object>> {
        &self.edits
    }
    /// Pure exact-view application. A changed view must replan; the caller still
    /// needs a real scoped ref/index fence to publish the returned root.
    pub fn apply(
        &self,
        current: &Tree,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> WorktreeResult<(Tree, TreeMetrics)> {
        if current.root() != &self.expected_root {
            return Err(WorktreeError::StaleTarget);
        }
        Ok(current.edit(&self.expected_root, &self.edits, store, limits)?)
    }
}

/// Share one metadata IO budget, including lookups ending in a shape conflict.
struct Budget {
    limits: TreeLimits,
    used: std::cell::RefCell<TreeMetrics>,
}
impl Budget {
    fn new(limits: TreeLimits) -> Self {
        Self {
            limits,
            used: std::cell::RefCell::new(TreeMetrics::default()),
        }
    }
    fn metrics(&self) -> TreeMetrics {
        self.used.borrow().clone()
    }
    fn get(
        &mut self,
        tree: &Tree,
        path: &LogicalPath,
        store: &mut impl NodeStore,
    ) -> WorktreeResult<Option<Object>> {
        let mut metered = Metered {
            inner: store,
            budget: self,
        };
        Ok(tree.get(path, &mut metered, self.limits)?.0)
    }
    fn edit(
        &mut self,
        tree: &Tree,
        changes: &BTreeMap<LogicalPath, Option<Object>>,
        store: &mut impl NodeStore,
    ) -> WorktreeResult<Tree> {
        let mut metered = Metered {
            inner: store,
            budget: self,
        };
        Ok(tree
            .edit(tree.root(), changes, &mut metered, self.limits)?
            .0)
    }
}
struct Metered<'a, S> {
    inner: &'a mut S,
    budget: &'a Budget,
}
impl<S: NodeStore> NodeStore for Metered<'_, S> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        let mut used = self.budget.used.borrow_mut();
        let limits = self.budget.limits;
        if used.reads >= limits.max_reads
            || object.bytes > limits.max_read_bytes.saturating_sub(used.read_bytes) as u64
        {
            return Err(TreeError::Budget);
        }
        used.reads += 1;
        used.read_bytes += object.bytes as usize;
        drop(used);
        self.inner.get(object)
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        let mut used = self.budget.used.borrow_mut();
        let limits = self.budget.limits;
        if used.writes >= limits.max_writes
            || bytes.len() > limits.max_write_bytes.saturating_sub(used.write_bytes)
        {
            return Err(TreeError::Budget);
        }
        used.writes += 1;
        used.write_bytes += bytes.len();
        drop(used);
        self.inner.put(bytes)
    }
}
