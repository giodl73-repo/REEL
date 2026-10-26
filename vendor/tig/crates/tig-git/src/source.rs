//! Bounded read-only extraction from exact Git commit/index views.
#[cfg(feature = "assets")]
pub mod native;
use crate::pointer::Pointer;
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tig_core::{
    Digest, LogicalPath, Object,
    tree::{NodeStore, Tree, TreeLimits},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Budget,
    Git,
    Io,
    Integrity,
    Incomplete,
    Metadata,
    Ambiguous,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

/// Git object identity, deliberately separate from raw Tig/LFS payload identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitId(String);
impl GitId {
    pub fn parse(s: &str) -> Result<Self> {
        if !matches!(s.len(), 40 | 64)
            || !s
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || s.bytes().all(|b| b == b'0')
        {
            return Err(Error::Invalid);
        }
        Ok(Self(s.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn verify_blob(&self, bytes: &[u8]) -> Result<()> {
        let header = format!("blob {}\0", bytes.len());
        let actual = if self.0.len() == 40 {
            let mut h = sha1::Sha1::new();
            h.update(header.as_bytes());
            h.update(bytes);
            h.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        } else {
            let mut h = Sha256::new();
            h.update(header.as_bytes());
            h.update(bytes);
            h.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        if actual != self.0 {
            return Err(Error::Integrity);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub listing_bytes: usize,
    pub entries: usize,
    pub pointer_blobs: usize,
    pub command_millis: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            listing_bytes: 8 * 1024 * 1024,
            entries: 32_768,
            pointer_blobs: 1024,
            command_millis: 5000,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<()> {
        if self.listing_bytes == 0
            || self.listing_bytes > 128 * 1024 * 1024
            || self.entries == 0
            || self.entries > 500_000
            || self.pointer_blobs == 0
            || self.pointer_blobs > 65_536
            || self.command_millis == 0
            || self.command_millis > 30_000
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Index,
    Commit(GitId),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub view: View,
    pub listing: Digest,
}
#[derive(Clone, Debug)]
struct Entry {
    path: String,
    oid: GitId,
    mode: String,
    stage: u8,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    enrollment: Digest,
    checkpoint: Checkpoint,
    entries: Vec<Entry>,
}
impl Snapshot {
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }
}

/// Explicit path scope, independent of mutable or inferred attributes.
#[derive(Clone, Debug)]
pub struct Scope {
    prefixes: BTreeSet<LogicalPath>,
}
impl Scope {
    pub fn prefixes(&self) -> impl Iterator<Item = &LogicalPath> {
        self.prefixes.iter()
    }
    pub fn new(prefixes: impl IntoIterator<Item = LogicalPath>) -> Result<Self> {
        let mut selected = BTreeSet::new();
        for (n, prefix) in prefixes.into_iter().enumerate() {
            if n >= 128 {
                return Err(Error::Budget);
            }
            selected.insert(prefix);
        }
        if selected.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { prefixes: selected })
    }
    fn contains(&self, path: &str) -> bool {
        self.prefixes.iter().any(|p| {
            path == p.as_str()
                || path
                    .strip_prefix(p.as_str())
                    .is_some_and(|s| s.starts_with('/'))
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueKind {
    InvalidPath,
    UnsupportedMode,
    Unmerged,
    MissingBlob,
    NotPointer,
    ConflictingSize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub stage: u8,
    pub kind: IssueKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub path: LogicalPath,
    pub stage: u8,
    pub object: Object,
}
#[derive(Clone, Debug)]
pub struct Extraction {
    checkpoint: Checkpoint,
    scope: Scope,
    bindings: Vec<Binding>,
    issues: Vec<Issue>,
}
impl Extraction {
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }
    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }
    pub fn complete(&self) -> bool {
        self.issues.is_empty()
    }
    /// Exact source/scope context for an admitted selection; not owner authority.
    pub fn selection_digest(&self) -> Result<Digest> {
        if !self.complete() {
            return Err(Error::Incomplete);
        }
        let view = match &self.checkpoint.view {
            View::Index => serde_json::json!({"kind":"index"}),
            View::Commit(id) => serde_json::json!({"kind":"commit","oid":id.as_str()}),
        };
        let value = serde_json::json!({"schema":"tig.git-selection.design-1","view":view,"listing":self.checkpoint.listing,"scope":self.scope.prefixes().map(|p|p.as_str()).collect::<Vec<_>>()});
        tig_core::identity::canonical_json(&value)
            .map(|bytes| Digest::blob(&bytes))
            .map_err(|_| Error::Budget)
    }
    /// Complete only for this exact source and explicit scope. Not claim adoption.
    pub fn write_tree(&self, nodes: &mut impl NodeStore, limits: TreeLimits) -> Result<Tree> {
        if !self.complete() {
            return Err(Error::Incomplete);
        }
        let bindings = self
            .bindings
            .iter()
            .map(|b| (b.path.clone(), b.object.clone()))
            .collect();
        build_tree(bindings, nodes, limits)
    }
    /// OID-keyed authorization for an explicit transfer profile, not retention.
    pub fn write_authority_tree(
        &self,
        nodes: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<Tree> {
        if !self.complete() {
            return Err(Error::Incomplete);
        }
        let bindings = self
            .bindings
            .iter()
            .map(|b| {
                Ok((
                    LogicalPath::parse(b.object.sha256.as_str()).map_err(|_| Error::Invalid)?,
                    b.object.clone(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        build_tree(bindings, nodes, limits)
    }
}
fn build_tree(
    bindings: BTreeMap<LogicalPath, Object>,
    nodes: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<Tree> {
    Tree::build(&bindings, nodes, limits)
        .map(|(t, _)| t)
        .map_err(|e| {
            if matches!(e, tig_core::tree::TreeError::Budget) {
                Error::Budget
            } else {
                Error::Metadata
            }
        })
}

/// Caller-enrolled paths. No discovery, configuration changes or payload IO.
pub struct Repository {
    enrollment: Digest,
    bare: bool,
    executable: PathBuf,
    root: PathBuf,
    limits: Limits,
}
impl Repository {
    pub fn new(executable: PathBuf, root: PathBuf, limits: Limits) -> Result<Self> {
        limits.validate()?;
        if !root.is_absolute() || !executable.is_absolute() {
            return Err(Error::Invalid);
        }
        let mut random = [0; 32];
        getrandom::fill(&mut random).map_err(|_| Error::Io)?;
        let mut repository = Self {
            enrollment: Digest::blob(&random),
            bare: false,
            executable,
            root,
            limits,
        };
        let prefix = repository.run(&["rev-parse", "--show-prefix"], Vec::new(), 4096)?;
        if !prefix.is_empty() && prefix != b"\n" {
            return Err(Error::Invalid);
        }
        repository.bare = match repository
            .run(&["rev-parse", "--is-bare-repository"], Vec::new(), 16)?
            .as_slice()
        {
            b"true\n" => true,
            b"false\n" => false,
            _ => return Err(Error::Integrity),
        };
        Ok(repository)
    }
    fn run(&self, args: &[&str], input: Vec<u8>, cap: usize) -> Result<Vec<u8>> {
        let mut c = Command::new(&self.executable);
        c.arg("--no-optional-locks")
            .arg("--no-replace-objects")
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.untrackedCache=false",
                "-C",
            ])
            .arg(&self.root)
            .args(args);
        // Inherited routing, injected configuration and transport settings cannot
        // silently substitute another repository or enable a promisor fetch.
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().to_ascii_uppercase().starts_with("GIT_") {
                c.env_remove(k);
            }
        }
        c.env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_ALLOW_PROTOCOL", "")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = c.spawn().map_err(|_| Error::Io)?;
        let mut stdin = child.stdin.take().ok_or(Error::Io)?;
        let stdout = child.stdout.take().ok_or(Error::Io)?;
        let writer = thread::spawn(move || stdin.write_all(&input));
        let reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(cap as u64 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let start = Instant::now();
        let status = loop {
            if start.elapsed() >= Duration::from_millis(self.limits.command_millis) {
                let _ = child.kill();
                let _ = child.wait();
                break Err(Error::Budget);
            }
            match child.try_wait() {
                Ok(Some(s)) => break Ok(s),
                Ok(None) => thread::sleep(Duration::from_millis(2)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(Error::Io);
                }
            }
        };
        let written = writer.join().map_err(|_| Error::Io)?;
        let bytes = reader
            .join()
            .map_err(|_| Error::Io)?
            .map_err(|_| Error::Io)?;
        if bytes.len() > cap {
            return Err(Error::Budget);
        }
        if !status?.success() {
            return Err(Error::Git);
        }
        written.map_err(|_| Error::Io)?;
        Ok(bytes)
    }
    pub fn index(&self) -> Result<Snapshot> {
        if self.bare {
            return Err(Error::Invalid);
        }
        let bytes = self.run(
            &["ls-files", "--stage", "-z"],
            Vec::new(),
            self.limits.listing_bytes,
        )?;
        self.parse_listing(View::Index, bytes)
    }
    pub fn commit(&self, id: GitId) -> Result<Snapshot> {
        let kind = self.run(&["cat-file", "-t", id.as_str()], Vec::new(), 16)?;
        if kind != b"commit\n" {
            return Err(Error::Invalid);
        }
        let bytes = self.run(
            &["ls-tree", "-r", "-z", "--full-tree", id.as_str()],
            Vec::new(),
            self.limits.listing_bytes,
        )?;
        self.parse_listing(View::Commit(id), bytes)
    }
    fn parse_listing(&self, view: View, bytes: Vec<u8>) -> Result<Snapshot> {
        let checkpoint = Checkpoint {
            view,
            listing: Digest::blob(&bytes),
        };
        if !bytes.is_empty() && bytes.last() != Some(&0) {
            return Err(Error::Integrity);
        }
        let mut entries = Vec::new();
        let mut seen = BTreeSet::new();
        for row in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            if entries.len() == self.limits.entries {
                return Err(Error::Budget);
            }
            let text = std::str::from_utf8(row).map_err(|_| Error::Invalid)?;
            let (meta, path) = text.split_once('\t').ok_or(Error::Integrity)?;
            if meta.len() > 128 {
                return Err(Error::Integrity);
            }
            if path.is_empty() || path.len() > 4096 {
                return Err(Error::Invalid);
            }
            let fields: Vec<_> = meta.split(' ').collect();
            if fields.len() != 3
                || fields[0].len() != 6
                || !fields[0].bytes().all(|b| (b'0'..=b'7').contains(&b))
            {
                return Err(Error::Integrity);
            }
            let (oid, stage) = match checkpoint.view {
                View::Index => {
                    let stage = fields[2].parse::<u8>().map_err(|_| Error::Integrity)?;
                    if stage > 3 {
                        return Err(Error::Integrity);
                    }
                    (GitId::parse(fields[1])?, stage)
                }
                View::Commit(_) => {
                    if !matches!(fields[1], "blob" | "commit") {
                        return Err(Error::Integrity);
                    }
                    (GitId::parse(fields[2])?, 0)
                }
            };
            if !seen.insert((path.to_owned(), stage)) {
                return Err(Error::Integrity);
            }
            entries.push(Entry {
                path: path.into(),
                oid,
                mode: fields[0].into(),
                stage,
            });
        }
        Ok(Snapshot {
            enrollment: self.enrollment.clone(),
            checkpoint,
            entries,
        })
    }
    fn blobs(&self, ids: BTreeSet<GitId>) -> Result<BTreeMap<GitId, Option<Vec<u8>>>> {
        self.read_blobs(ids, 1024, self.limits.pointer_blobs * 1024, false)
    }
    fn read_blobs(
        &self,
        ids: BTreeSet<GitId>,
        max_blob: usize,
        max_bytes: usize,
        strict: bool,
    ) -> Result<BTreeMap<GitId, Option<Vec<u8>>>> {
        if ids.len() > self.limits.pointer_blobs {
            return Err(Error::Budget);
        }
        if ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let input = |ids: &BTreeSet<GitId>| {
            ids.iter()
                .flat_map(|id| id.0.bytes().chain(*b"\n"))
                .collect::<Vec<_>>()
        };
        let sizes = self.run(&["cat-file", "--batch-check"], input(&ids), ids.len() * 128)?;
        let text = std::str::from_utf8(&sizes).map_err(|_| Error::Integrity)?;
        let mut result = BTreeMap::new();
        let mut small = BTreeSet::new();
        let mut total = 0usize;
        let rows: Vec<_> = text.lines().collect();
        if rows.len() != ids.len() {
            return Err(Error::Integrity);
        }
        for (id, row) in ids.iter().zip(rows) {
            let fields: Vec<_> = row.split(' ').collect();
            if fields.first().copied() != Some(id.as_str()) {
                return Err(Error::Integrity);
            }
            if fields.len() == 2 && fields[1] == "missing" {
                result.insert(id.clone(), None);
                continue;
            }
            if fields.len() != 3 || fields[1] != "blob" {
                return Err(Error::Integrity);
            }
            let size = fields[2].parse::<u64>().map_err(|_| Error::Integrity)?;
            if size <= max_blob as u64 {
                total = total
                    .checked_add(size as usize)
                    .filter(|n| *n <= max_bytes)
                    .ok_or(Error::Budget)?;
                small.insert(id.clone());
            } else if strict {
                return Err(Error::Budget);
            }
            // Oversize entries are not pointer bytes. Empty sentinel cannot parse.
            result.insert(id.clone(), Some(Vec::new()));
        }
        if small.is_empty() {
            return Ok(result);
        }
        let cap = total.checked_add(small.len() * 128).ok_or(Error::Budget)?;
        let bytes = self.run(&["cat-file", "--batch"], input(&small), cap)?;
        let mut rest = bytes.as_slice();
        for id in small {
            let end = rest
                .iter()
                .position(|b| *b == b'\n')
                .ok_or(Error::Integrity)?;
            let header = std::str::from_utf8(&rest[..end]).map_err(|_| Error::Integrity)?;
            let fields: Vec<_> = header.split(' ').collect();
            if fields.len() != 3 || fields[0] != id.as_str() || fields[1] != "blob" {
                return Err(Error::Integrity);
            }
            let size = fields[2].parse::<usize>().map_err(|_| Error::Integrity)?;
            rest = &rest[end + 1..];
            if size > max_blob || rest.len() <= size || rest[size] != b'\n' {
                return Err(Error::Integrity);
            }
            let content = rest[..size].to_vec();
            id.verify_blob(&content)?;
            rest = &rest[size + 1..];
            result.insert(id, Some(content));
        }
        if !rest.is_empty() {
            return Err(Error::Integrity);
        }
        Ok(result)
    }
    pub fn extract(&self, snapshot: &Snapshot, scope: Scope) -> Result<Extraction> {
        if snapshot.enrollment != self.enrollment {
            return Err(Error::Invalid);
        }
        let selected: Vec<_> = snapshot
            .entries
            .iter()
            .filter(|e| scope.contains(&e.path))
            .collect();
        if selected.len() > self.limits.entries {
            return Err(Error::Budget);
        }
        let ids = selected
            .iter()
            .filter(|e| matches!(e.mode.as_str(), "100644" | "100755"))
            .map(|e| e.oid.clone())
            .collect();
        let blobs = self.blobs(ids)?;
        let mut out = Extraction {
            checkpoint: snapshot.checkpoint.clone(),
            scope,
            bindings: Vec::new(),
            issues: Vec::new(),
        };
        let mut sizes = BTreeMap::new();
        for e in selected {
            let mut issue = |kind| {
                out.issues.push(Issue {
                    path: e.path.clone(),
                    stage: e.stage,
                    kind,
                })
            };
            if e.stage != 0 {
                issue(IssueKind::Unmerged);
            }
            let Ok(path) = LogicalPath::parse(&e.path) else {
                issue(IssueKind::InvalidPath);
                continue;
            };
            if !matches!(e.mode.as_str(), "100644" | "100755") {
                issue(IssueKind::UnsupportedMode);
                continue;
            }
            let Some(bytes) = &blobs[&e.oid] else {
                issue(IssueKind::MissingBlob);
                continue;
            };
            let Ok(pointer) = Pointer::parse(bytes) else {
                issue(IssueKind::NotPointer);
                continue;
            };
            let object = pointer.object().clone();
            if sizes
                .insert(object.sha256.clone(), object.bytes)
                .is_some_and(|n| n != object.bytes)
            {
                issue(IssueKind::ConflictingSize);
            }
            out.bindings.push(Binding {
                path,
                stage: e.stage,
                object,
            });
        }
        Ok(out)
    }
}
