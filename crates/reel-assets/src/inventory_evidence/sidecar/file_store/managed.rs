//! Explicit managed production and read-only query parity. No census refresh,
//! rendering, automatic catalog creation, retirement or error recovery.
use super::*;
use crate::inventory_evidence as evidence;
use catalog::{Candidate, Catalog, MutationContext};
pub mod command;

#[derive(Clone, Copy, Default)]
pub struct Budgets {
    pub preparation: evidence::Limits,
    pub index: BuildLimits,
}
impl Budgets {
    fn store(self) -> Limits {
        Limits {
            node_bytes: self.index.node_bytes,
            staged_bytes: self.index.total_bytes,
            nodes: self.index.nodes,
        }
    }
    fn whole(self) -> ReadLimits {
        ReadLimits {
            node_bytes: self.index.node_bytes,
            total_bytes: self.index.total_bytes,
            nodes: self.index.nodes,
            rows: self.preparation.objects,
        }
    }
    fn validate(self) -> Result<()> {
        check_limits(self.store())?;
        if self.index.leaf_rows == 0
            || self.index.leaf_rows > 4096
            || self.preparation.encoded_bytes > 512 * 1024 * 1024
            || self.preparation.decoded_bytes > 1024 * 1024 * 1024
            || self.preparation.objects > 1_000_000
            || self.preparation.total_copies > 4_000_000
        {
            return Err("Unsupported managed inventory budget".into());
        }
        Ok(())
    }
}
/// Input paths are explicit existing cache locations, never producer-supplied
/// paths extracted from the inventory body. Source custody remains external.
pub struct Request<'a> {
    pub catalog: &'a Catalog,
    pub cache: &'a Path,
    pub source_cache: &'a Path,
    pub staging_parent: &'a Path,
    pub retained_parent: &'a Path,
    pub key: &'a str,
    pub session_id: &'a str,
    pub expected_revision: u64,
    pub source: &'a Source,
    pub budgets: Budgets,
    pub staging_overhead_bytes: u64,
}
/// Private exact result; automatic report projection is deliberately count-only.
pub struct Produced {
    pub root: Blob,
    summary: BuildSummary,
}
impl Produced {
    pub fn summary(&self) -> &BuildSummary {
        &self.summary
    }
}
#[derive(Serialize)]
pub struct BuildSummary {
    pub schema: &'static str,
    pub objects: usize,
    pub copies: usize,
    pub index_nodes: usize,
    pub index_bytes: u64,
    pub catalog_revision: u64,
    pub staging_retained: bool,
    pub media_payloads_copied: u64,
    pub removal_authorized: bool,
}
fn source_reader(cache: &Path, source: &Source, limits: evidence::Limits) -> Result<File> {
    if !valid_oid(&source.encoded.sha256) || source.encoded.bytes > limits.encoded_bytes {
        return Err("Invalid named inventory source descriptor/budget".into());
    }
    let cache = checked_directory(cache)?;
    let path = crate::object(&cache, &source.encoded.sha256);
    checked_directory(path.parent().ok_or("Missing named source parent")?)?;
    if plain(&path, false)?.len() != source.encoded.bytes {
        return Err("Named source length mismatch".into());
    }
    Ok(File::open(path)?)
}
/// Build -> seal -> retain -> verify candidate -> expected-revision admission.
/// Failure may leave protected staging/retained nodes or a pending catalog
/// decision. No failure handler deletes, retries or recovers on the caller's behalf.
pub fn build(context: &MutationContext<'_>, request: &Request<'_>) -> Result<Produced> {
    build_protocol(context, request, None)
}
/// Opt-in node-publication enrollment. Original-source demand and other
/// producer families still need their separate lifecycle contracts.
pub fn build_fenced(
    context: &MutationContext<'_>,
    request: &Request<'_>,
    domain: &collector::fence::Domain,
) -> Result<Produced> {
    domain.binding(&checked_directory(request.cache)?)?;
    build_protocol(context, request, Some(domain))
}
fn build_protocol(
    context: &MutationContext<'_>,
    request: &Request<'_>,
    domain: Option<&collector::fence::Domain>,
) -> Result<Produced> {
    request.budgets.validate()?;
    FileStore::capacity_budget(request.budgets.store(), request.staging_overhead_bytes)?;
    request.catalog.producer_preflight(
        request.cache,
        request.source,
        request.key,
        request.expected_revision,
    )?;
    let index = evidence::prepare(
        source_reader(
            request.source_cache,
            request.source,
            request.budgets.preparation,
        )?,
        request.source,
        request.budgets.preparation,
    )?;
    let mut writer = if let Some(domain) = domain {
        FileStore::begin_admitted_fenced(
            context.project_root,
            context.storage,
            request.cache,
            request.staging_parent,
            request.session_id,
            request.budgets.store(),
            request.staging_overhead_bytes,
            domain,
        )?
    } else {
        FileStore::begin_admitted(
            context.project_root,
            context.storage,
            request.cache,
            request.staging_parent,
            request.session_id,
            request.budgets.store(),
            request.staging_overhead_bytes,
        )?
    };
    let root = super::super::publish(&mut writer, &index, request.budgets.index)?;
    writer.seal(&root)?;
    writer.handoff(request.retained_parent, &root, request.source)?;
    let candidate = Candidate::verify(
        request.cache,
        request.retained_parent,
        &root,
        request.source,
        request.budgets.whole(),
    )?;
    let selected =
        request
            .catalog
            .admit(context, request.key, request.expected_revision, &candidate)?;
    let summary = BuildSummary {
        schema: "reel.inventory-index-build-summary.v1",
        objects: index.summary().objects,
        copies: index.summary().copies,
        index_nodes: writer.staged_nodes(),
        index_bytes: writer.staged_bytes(),
        catalog_revision: selected.revision,
        staging_retained: true,
        media_payloads_copied: 0,
        removal_authorized: false,
    };
    Ok(Produced { root, summary })
}

#[derive(Clone, Copy)]
pub struct ShadowLimits {
    pub subjects: usize,
    pub node_bytes: u64,
    pub total_bytes: u64,
    pub nodes: usize,
}
impl Default for ShadowLimits {
    fn default() -> Self {
        Self {
            subjects: 4096,
            node_bytes: 256 * 1024,
            total_bytes: 256 * 1024 * 1024,
            nodes: 16384,
        }
    }
}
#[derive(Serialize)]
pub struct ShadowSummary {
    pub schema: &'static str,
    pub subjects_checked: usize,
    pub listed: usize,
    pub not_listed_at_capture: usize,
    pub index_nodes_read: usize,
    pub index_bytes_read: u64,
    pub catalog_revision: u64,
    pub parity: bool,
    pub writes: u64,
    pub removal_authorized: bool,
}
fn parity_metadata(index: &PreparedInventory, found: &IndexedLookup) -> bool {
    index.capture().map(|c| (c.started, c.completed))
        == found.capture.map(|c| (c.started, c.completed))
        && index.profile_sha256() == found.profile_sha256
        && index.registry_generation() == found.registry_generation
        && index.scope() == found.scope.as_deref()
        && index.named_scope() == found.named_scope.as_deref()
        && index.producer_dependency_coverage() == found.producer_dependency_coverage
        && index
            .tiers
            .values()
            .map(|t| (t.tier, &t.root, &t.volume_id))
            .eq(found.tiers.iter().map(|t| (t.tier, &t.root, &t.volume_id)))
}
/// Compare requested exact lookups against the verified original native source.
/// Absence remains captured nonmembership. This is not planner-decision parity
/// or custody validation of inventory-listed media. All source/index I/O is named.
pub fn shadow(
    request: &Request<'_>,
    subjects: &[String],
    limits: ShadowLimits,
) -> Result<ShadowSummary> {
    request.budgets.validate()?;
    check_node_limit(limits.node_bytes)?;
    if subjects.is_empty()
        || subjects.len() > limits.subjects
        || limits.nodes == 0
        || limits.total_bytes == 0
        || limits.subjects > 100_000
        || limits.nodes > 1_000_000
        || limits.total_bytes > 4 * 1024 * 1024 * 1024
        || subjects.iter().any(|h| !valid_oid(h))
        || subjects.iter().collect::<BTreeSet<_>>().len() != subjects.len()
    {
        return Err("Invalid or over-budget exact shadow subjects".into());
    }
    let selected = request.catalog.read(request.key)?;
    let root = selected
        .root()
        .ok_or("No catalog index selected for shadow comparison")?;
    let source = selected.source()?.ok_or("Missing selected index source")?;
    if source_key(&source) != source_key(request.source) {
        return Err("Shadow source differs from selected catalog evidence".into());
    }
    request.catalog.producer_preflight(
        request.cache,
        request.source,
        request.key,
        selected.revision,
    )?;
    let original = evidence::prepare(
        source_reader(
            request.source_cache,
            request.source,
            request.budgets.preparation,
        )?,
        request.source,
        request.budgets.preparation,
    )?;
    let mut reader = ReadOnlyStore::new(request.cache, limits.node_bytes)?;
    let mut nodes = 0usize;
    let mut bytes = 0u64;
    let mut listed = 0usize;
    for subject in subjects {
        let found = super::super::lookup(
            &mut reader,
            &root,
            &source,
            subject,
            ReadLimits {
                node_bytes: limits.node_bytes,
                total_bytes: (limits.total_bytes - bytes).min(8 * 1024 * 1024),
                nodes: (limits.nodes - nodes).min(66),
                rows: 4096,
            },
        )?;
        nodes = nodes
            .checked_add(found.nodes_read)
            .ok_or("Shadow node overflow")?;
        bytes = bytes
            .checked_add(found.bytes_read)
            .ok_or("Shadow byte overflow")?;
        if !parity_metadata(&original, &found) {
            return Err("Inventory shadow scope/metadata parity mismatch".into());
        }
        match (original.lookup(subject)?, found.observation) {
            (evidence::Lookup::Listed(expected), Some(actual))
                if observation(expected) == observation(&actual)
                    && expected.source_record == actual.source_record =>
            {
                listed += 1
            }
            (evidence::Lookup::NotListedAtCapture, None) => (),
            _ => return Err("Inventory shadow row/copy/witness parity mismatch".into()),
        }
    }
    let current = request.catalog.read(request.key)?;
    if current.revision != selected.revision || current.root() != Some(root) {
        return Err("Catalog changed during shadow comparison; retry".into());
    }
    Ok(ShadowSummary {
        schema: "reel.inventory-index-shadow-summary.v1",
        subjects_checked: subjects.len(),
        listed,
        not_listed_at_capture: subjects.len() - listed,
        index_nodes_read: nodes,
        index_bytes_read: bytes,
        catalog_revision: selected.revision,
        parity: true,
        writes: 0,
        removal_authorized: false,
    })
}
