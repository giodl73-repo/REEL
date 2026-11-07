//! One captured source, bounded candidates and one final sparse Git proposal.
use super::*;
use std::collections::BTreeSet;
use tig_assets::{edits::Prepared, resolve::AssetResolver};
use tig_core::{Digest, identity};

const ITEMS: usize = 64;
const OPERATIONS: usize = 256;
const DOCUMENTS: usize = 4096;
const METADATA_BYTES: usize = 64 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchProposal {
    identity: Identity,
    message: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    git_executable: PathBuf,
    repository: PathBuf,
    base: String,
    records: Vec<String>,
    definitions: Vec<String>,
    dialect: String,
    #[serde(default)]
    limits: CaptureLimits,
    byte_source: Option<ByteProfile>,
    publication: Option<Publication>,
    proposal: Option<BatchProposal>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    schema: String,
    items: Vec<Item>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    asset: String,
    path: String,
    expected: Object,
    request: Request,
}
struct Work {
    asset: String,
    path: LogicalPath,
    expected: Object,
    loaded: changes::Loaded,
}

fn operation(namespace: &Key, asset: &str) -> Result<Key, String> {
    let raw = identity::canonical_json(
        &json!({"schema":"tig.history-batch-item-operation.experimental-1",
        "namespace":namespace,"asset":asset}),
    )
    .map_err(|_| "invalid batch operation")?;
    Key::parse(&format!("history-batch-{}", Digest::blob(&raw).as_str()))
        .map_err(|_| "invalid batch operation".into())
}
fn replacement(work: &Work, prepared: &Prepared) -> Replacement {
    Replacement {
        path: work.path.clone(),
        expected: prepared.expected().clone(),
        bytes: prepared.record().bytes().to_vec(),
    }
}
pub(super) fn execute<B: PublicationBackend>(
    action: &str,
    profile_path: &Path,
    request_path: &Path,
    open: impl FnOnce(&Publication) -> Result<B, publication::Error>,
) -> Result<Value, String> {
    if !matches!(action, "check" | "publish" | "propose") {
        return Err("invalid history batch action".into());
    }
    let mut profile: Profile = input(profile_path)?;
    let batch: Batch = input(request_path)?;
    if profile.schema != "tig.history-batch-profile.experimental-1"
        || batch.schema != "tig.history-batch-request.experimental-1"
    {
        return Err("unsupported history batch schema".into());
    }
    if batch.items.is_empty() || batch.items.len() > ITEMS {
        return Err("history batch item budget".into());
    }
    let operations = batch
        .items
        .iter()
        .map(|item| match &item.request {
            Request::Sequence { changes } => changes.len(),
            _ => 1,
        })
        .sum::<usize>();
    if operations > OPERATIONS {
        return Err("history batch operation budget".into());
    }
    if action != "check" && profile.publication.is_none() {
        return Err("explicit publication profile required".into());
    }
    if action == "propose" && profile.proposal.is_none() {
        return Err("explicit proposal profile required".into());
    }
    if action != "check"
        && !profile.publication.as_ref().is_some_and(|p| {
            profile.byte_source.as_ref().is_some_and(|b| {
                b.places.iter().any(|place| {
                    place.root == p.root
                        && std::mem::discriminant(&place.layout)
                            == std::mem::discriminant(&p.layout)
                })
            })
        })
    {
        return Err("publication root/layout must be an explicit reader place".into());
    }
    let dialect = profile
        .dialect
        .parse::<Dialect>()
        .map_err(|_| "invalid schema dialect")?;
    let records = scope(profile.records)?;
    let mut paths = BTreeSet::new();
    let mut assets = BTreeSet::new();
    let mut declarations = Vec::new();
    for item in batch.items {
        let path = LogicalPath::parse(&item.path).map_err(|_| "invalid batch record path")?;
        if item.asset.is_empty()
            || item.asset.len() > 256
            || item.asset.chars().any(char::is_control)
            || !assets.insert(item.asset.clone())
            || !paths.insert(path.clone())
        {
            return Err("duplicate or invalid batch identity/path".into());
        }
        if !records.prefixes().any(|p| {
            path.as_str() == p.as_str()
                || path
                    .as_str()
                    .strip_prefix(p.as_str())
                    .is_some_and(|s| s.starts_with('/'))
        }) {
            return Err("batch path outside declared record scope".into());
        }
        declarations.push((item, path));
    }
    let mut work = Vec::new();
    for (item, path) in declarations {
        work.push(Work {
            asset: item.asset,
            path,
            expected: item.expected,
            loaded: changes::Loaded::read(item.request, request_path)?,
        });
    }
    if let Some(bytes) = &mut profile.byte_source {
        bytes.limits.total_bytes = bytes.limits.total_bytes.min(METADATA_BYTES as u64);
        bytes.limits.requests = bytes.limits.requests.min(DOCUMENTS as u32);
        bytes.limits.object_bytes = bytes.limits.object_bytes.min(16 * 1024 * 1024);
    }
    profile.limits.bytes = profile.limits.bytes.min(16 * 1024 * 1024);
    profile.limits.documents = profile.limits.documents.min(16_384);
    let mut reader = CacheMetadataReader::new(profile.byte_source)
        .map_err(|e| format!("history byte profile: {e}"))?;
    let repository = Repository::new(
        profile.git_executable,
        profile.repository,
        GitLimits::default(),
    )
    .map_err(|e| e.to_string())?;
    let base = GitId::parse(&profile.base).map_err(|e| e.to_string())?;
    let view = repository.commit(base.clone()).map_err(|e| e.to_string())?;
    for item in &work {
        let raw = repository
            .document(&view, &item.path)
            .map_err(|_| "batch path unavailable")?;
        if Object::from_bytes(&raw) != item.expected {
            return Err("stale batch path".into());
        }
    }
    let snapshot = repository
        .native(
            &view,
            Scope::new(paths).map_err(|e| e.to_string())?,
            scope(profile.definitions)?,
            profile.limits,
        )
        .map_err(|e| e.to_string())?;
    let mut prepared = Vec::new();
    let mut documents = 0usize;
    let mut bytes = 0usize;
    for item in &work {
        // ID and expected bytes must be the record at this exact declared path.
        let asset = snapshot
            .resolve_id(&item.asset)
            .map_err(|_| "batch asset unavailable")?;
        if asset.reference().object != item.expected {
            return Err("stale batch record".into());
        }
        let candidate = item
            .loaded
            .prepare(&snapshot, &item.asset, &item.expected, dialect, &mut reader)
            .map_err(|e| {
                reader
                    .take_failure()
                    .unwrap_or_else(|| format!("history preparation: {e}"))
            })?;
        for blob in candidate.metadata().iter().chain([candidate.record()]) {
            documents = documents
                .checked_add(1)
                .ok_or("history batch metadata budget")?;
            bytes = bytes
                .checked_add(blob.bytes().len())
                .ok_or("history batch metadata budget")?;
            if documents > DOCUMENTS || bytes > METADATA_BYTES {
                return Err("history batch metadata budget".into());
            }
        }
        prepared.push(candidate);
    }
    let replacements = work
        .iter()
        .zip(&prepared)
        .map(|(w, p)| replacement(w, p))
        .collect::<Vec<_>>();
    if let Some(proposal) = &profile.proposal {
        repository
            .check_proposal(
                base.clone(),
                &replacements,
                &proposal.identity,
                &proposal.message,
            )
            .map_err(|e| format!("proposal preflight: {e}"))?;
    }
    let noop = prepared.iter().all(Prepared::is_noop);
    let mut output = json!({"schema":"tig.history-batch-result.experimental-1","action":action,
        "base":base.as_str(),"source_checkpoint":snapshot.checkpoint(),"noop":noop,"published":false,
        "new_metadata_readback_verified":false,"proposal":null,"items":prepared.iter().enumerate().map(|(index,p)|
            json!({"ordinal":index,"expected":p.expected(),"record":p.record().object(),"noop":p.is_noop(),
                "metadata":p.metadata().iter().map(|b|b.object()).collect::<Vec<_>>(),"intent":null})).collect::<Vec<_>>(),
        "approval":false,"current_selection":false,"old_protection_released":false,
        "independent_custody_verified":false,"complete_semantic_coverage":false});
    if action == "check" || noop {
        return Ok(output);
    }
    let publication = profile
        .publication
        .as_ref()
        .ok_or("publication profile required")?;
    let operations = work
        .iter()
        .map(|w| operation(&publication.operation, &w.asset))
        .collect::<Result<Vec<_>, _>>()?;
    let mut backend = open(publication).map_err(|e| format!("publication enrollment: {e}"))?;
    let capabilities = backend.capabilities();
    for (item, operation) in prepared.iter().zip(&operations) {
        publication::preflight(
            item,
            operation,
            &publication.owner,
            publication.required,
            capabilities,
        )
        .map_err(|e| format!("history publication preflight: {e}"))?;
    }
    for (index, (item, operation)) in prepared.iter().zip(&operations).enumerate() {
        let published = publication::publish(
            item,
            operation,
            &publication.owner,
            publication.required,
            &mut backend,
        )
        .map_err(|e| format!("history publication: {e}"))?;
        output["items"][index]["intent"] = json!(published.intent());
    }
    for item in prepared.iter().filter(|p| !p.is_noop()) {
        for blob in item.metadata().iter().chain([item.record()]) {
            let raw = reader.read(blob.object(), 65_536).map_err(|e| {
                reader
                    .take_failure()
                    .unwrap_or_else(|| format!("published metadata readback: {e}"))
            })?;
            if Object::from_bytes(&raw) != *blob.object() {
                return Err("published metadata readback differs".into());
            }
        }
    }
    output["published"] = json!(true);
    output["new_metadata_readback_verified"] = json!(true);
    if action == "propose" {
        let proposal = profile
            .proposal
            .as_ref()
            .ok_or("proposal profile required")?;
        let proposal = repository
            .propose(base, &replacements, &proposal.identity, &proposal.message)
            .map_err(|e| format!("proposal creation: {e}"))?;
        output["proposal"] =
            json!({"commit":proposal.commit().as_str(),"reference":proposal.reference()});
    }
    Ok(output)
}
