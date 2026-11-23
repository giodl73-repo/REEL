//! Explicit bounded provider copy. Caller owns creation, capacity and adoption.
use super::*;
use sha2::{Digest as _, Sha256};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub nodes: u64,
    pub bytes: u64,
    pub millis: u64,
}
impl Budget {
    /// Validate finite copy/verification traversal ceilings.
    pub fn validate(&self) -> Result<()> {
        if self.nodes == 0
            || self.nodes > 2_000_000
            || self.bytes == 0
            || self.bytes > 2 * 1024 * 1024 * 1024
            || self.millis == 0
            || self.millis > 600_000
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
    fn timed(&self, start: Instant) -> Result<()> {
        if start.elapsed() > Duration::from_millis(self.millis) {
            return Err(Error::Budget);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub source: Checkpoint,
    pub target: Checkpoint,
    pub source_limits: Limits,
    pub target_limits: Limits,
    pub nodes: u64,
    pub bytes: u64,
    pub object_set: Digest,
    pub fresh_target_readback_verified: bool,
}

struct SetHash(Sha256);
impl SetHash {
    fn new() -> Self {
        let mut hash = Sha256::new();
        hash.update(b"tig.metadata-provider-copy-set.v1\0");
        Self(hash)
    }
    fn push(&mut self, object: &Object) {
        self.0.update(object.sha256.as_str().as_bytes());
        self.0.update(object.bytes.to_be_bytes());
    }
    fn finish(self) -> Result<Digest> {
        let hex = self
            .0
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Digest::parse(&hex).map_err(|_| Error::Corrupt)
    }
}

fn page(reader: &MetadataReader, after: Option<&Digest>) -> Result<Vec<Object>> {
    let mut statement = reader.conn.prepare_cached(if after.is_some() {
        "SELECT oid,size,length(body) FROM nodes WHERE oid>?1 ORDER BY oid LIMIT ?2"
    } else {
        "SELECT oid,size,length(body) FROM nodes ORDER BY oid LIMIT ?1"
    })?;
    let mut rows = if let Some(after) = after {
        statement.query(params![after.as_str(), BATCH_NODES as i64])?
    } else {
        statement.query(params![BATCH_NODES as i64])?
    };
    let mut objects = Vec::new();
    while let Some(row) = rows.next()? {
        let oid: String = row.get(0)?;
        let size: i64 = row.get(1)?;
        let actual: i64 = row.get(2)?;
        if size < 0 || size != actual || size as u64 > MAX_NODE_BYTES as u64 {
            return Err(Error::Corrupt);
        }
        let digest = Digest::parse(&oid).map_err(|_| Error::Corrupt)?;
        if after.is_some_and(|after| digest.as_str() <= after.as_str())
            || objects
                .last()
                .is_some_and(|last: &Object| digest.as_str() <= last.sha256.as_str())
        {
            return Err(Error::Corrupt);
        }
        objects.push(Object {
            sha256: digest,
            bytes: size as u64,
        });
    }
    Ok(objects)
}

fn get(reader: &MetadataReader, object: &Object) -> Result<Vec<u8>> {
    let raw = reader
        .get(object)
        .map_err(|error| match error {
            TreeError::Budget => Error::Budget,
            _ => Error::Corrupt,
        })?
        .ok_or(Error::Corrupt)?;
    if Object::from_bytes(&raw) != *object {
        return Err(Error::Corrupt);
    }
    Ok(raw)
}

fn scan(
    reader: &MetadataReader,
    budget: Budget,
    start: Instant,
    mut visit: impl FnMut(&Object, &[u8]) -> Result<()>,
) -> Result<(u64, u64, Digest)> {
    let mut after = None;
    let mut count = 0u64;
    let mut bytes = 0u64;
    let mut digest = SetHash::new();
    loop {
        budget.timed(start)?;
        let objects = page(reader, after.as_ref())?;
        if objects.is_empty() {
            break;
        }
        for object in objects {
            budget.timed(start)?;
            count = count.checked_add(1).ok_or(Error::Budget)?;
            bytes = bytes.checked_add(object.bytes).ok_or(Error::Budget)?;
            if count > budget.nodes || bytes > budget.bytes {
                return Err(Error::Budget);
            }
            let raw = get(reader, &object)?;
            visit(&object, &raw)?;
            digest.push(&object);
            after = Some(object.sha256);
        }
    }
    if count != reader.checkpoint().nodes || bytes != reader.checkpoint().bytes {
        return Err(Error::Corrupt);
    }
    Ok((count, bytes, digest.finish()?))
}

#[derive(Clone, Copy)]
enum Phase {
    CopiedNode,
    BeforeSourceRecheck,
    AfterTargetFinish,
}

fn copy_inner(
    source_root: &Path,
    minimum: &Checkpoint,
    target_root: &Path,
    target: &mut MetadataStore,
    budget: Budget,
    mut hook: impl FnMut(Phase) -> Result<()>,
) -> Result<Observation> {
    budget.validate()?;
    let start = Instant::now();
    let source_path = fs::canonicalize(source_root)?;
    let target_path = fs::canonicalize(target_root)?;
    if source_path.starts_with(&target_path) || target_path.starts_with(&source_path) {
        return Err(Error::Foreign);
    }
    let source = MetadataReader::open(source_root, minimum)?;
    let baseline = source.checkpoint().clone();
    if &baseline != minimum {
        return Err(Error::Foreign);
    }
    let initial = target.checkpoint();
    if initial.identity == baseline.identity
        || initial.nodes != 0
        || initial.bytes != 0
        || !target.pending.is_empty()
        || target.poisoned
    {
        return Err(Error::Foreign);
    }
    let declared = MetadataReader::open(target_root, &initial)?;
    if declared.checkpoint() != &initial || declared.limits() != target.limits() {
        return Err(Error::Foreign);
    }
    drop(declared);
    if baseline.nodes > budget.nodes
        || baseline.bytes > budget.bytes
        || baseline.nodes > target.limits().nodes
        || baseline.bytes > target.limits().bytes
    {
        return Err(Error::Budget);
    }
    let scanned = scan(&source, budget, start, |object, raw| {
        if target.put(raw).map_err(|error| match error {
            TreeError::Budget => Error::Budget,
            _ => Error::Corrupt,
        })? != *object
        {
            return Err(Error::Corrupt);
        }
        hook(Phase::CopiedNode)
    });
    hook(Phase::BeforeSourceRecheck)?;
    let observed = MetadataReader::open(source_root, &baseline)?;
    if observed.checkpoint() != &baseline || observed.limits() != source.limits() {
        return Err(Error::Foreign);
    }
    let (nodes, bytes, object_set) = scanned?;
    let finished = target.finish()?;
    if finished.nodes != nodes || finished.bytes != bytes {
        return Err(Error::Corrupt);
    }
    hook(Phase::AfterTargetFinish)?;
    let fresh = MetadataReader::open(target_root, &finished)?;
    if fresh.checkpoint() != &finished || fresh.limits() != target.limits() {
        return Err(Error::Foreign);
    }
    let verified = scan(&fresh, budget, start, |_, _| Ok(()));
    let source_end = MetadataReader::open(source_root, &baseline)?;
    let target_end = MetadataReader::open(target_root, &finished)?;
    if source_end.checkpoint() != &baseline
        || source_end.limits() != source.limits()
        || target_end.checkpoint() != &finished
        || target_end.limits() != target.limits()
    {
        return Err(Error::Foreign);
    }
    if verified? != (nodes, bytes, object_set.clone()) {
        return Err(Error::Corrupt);
    }
    Ok(Observation {
        source: baseline,
        target: finished,
        source_limits: source.limits(),
        target_limits: target.limits(),
        nodes,
        bytes,
        object_set,
        fresh_target_readback_verified: true,
    })
}

/// Copy one stable observed provider into a caller-created empty provider.
/// The source checkpoint must match exactly; a newer source requires replanning.
/// No provider creation, source writes, journal change, adoption or removal.
/// Caller must admit physical capacity and preserve an unselected target on error.
pub fn copy_to_empty(
    source_root: &Path,
    minimum: &Checkpoint,
    target_root: &Path,
    target: &mut MetadataStore,
    budget: Budget,
) -> Result<Observation> {
    copy_inner(
        source_root,
        minimum,
        target_root,
        target,
        budget,
        |_| Ok(()),
    )
}

/// Independently reopen a target against a caller-admitted copy observation.
/// This verifies the observed target set, not a globally current source or custody.
pub fn verify_target(target_root: &Path, observation: &Observation, budget: Budget) -> Result<()> {
    budget.validate()?;
    let start = Instant::now();
    let reader = MetadataReader::open(target_root, &observation.target)?;
    if reader.checkpoint() != &observation.target || reader.limits() != observation.target_limits {
        return Err(Error::Foreign);
    }
    let verified = scan(&reader, budget, start, |_, _| Ok(()));
    let end = MetadataReader::open(target_root, &observation.target)?;
    if end.checkpoint() != reader.checkpoint() || end.limits() != reader.limits() {
        return Err(Error::Foreign);
    }
    if verified?
        != (
            observation.nodes,
            observation.bytes,
            observation.object_set.clone(),
        )
    {
        return Err(Error::Corrupt);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
