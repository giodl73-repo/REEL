//! Metadata-only recovery admission. No runtime binding or compiler replay.
use super::*;
use serde::Deserialize;
use std::{fs::File, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema: String,
    job_id: String,
    original_input: Selection,
    #[serde(default)]
    published_result: Option<Selection>,
    cache: Config,
    coordination_root: PathBuf,
    journal_root: PathBuf,
    journal_minimum: tig_store::source_claims::Checkpoint,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    source_id: String,
    owner: String,
    expected_revision: u64,
    retain_operation: String,
    budget: RecoveryBudget,
}

/// Complete only a recorded prepare, using existing stores and exact input pins.
/// This verifies ownership and bytes; it does not recreate a native producer
/// assertion, execute tools, finish child handoff or authorize workspace removal.
pub fn adopt_prepared(path: &Path) -> Result<Value> {
    execute(path, false)
}

/// Explicitly retain a complete published result when prepare was not recorded.
/// This creates the original prepare only after full guarded packet verification.
pub fn retain_published(path: &Path) -> Result<Value> {
    execute(path, true)
}

fn execute(path: &Path, published: bool) -> Result<Value> {
    let mut raw = Vec::new();
    File::open(path)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > 128 * 1024 {
        bail!("prepared recovery profile exceeds bound");
    }
    let a: Admission = serde_json::from_value(identity::parse_json(&raw)?)?;
    if a.schema
        != if published {
            "reel.tig-published-recovery-profile.experimental-1"
        } else {
            "reel.tig-prepared-recovery-profile.experimental-1"
        }
        || a.published_result.is_some() != published
        || !a.coordination_root.is_absolute()
        || !a.coordination_root.is_dir()
        || !a.journal_root.is_absolute()
        || !a.metadata_root.is_absolute()
        || a.cache.cache_roots.is_empty()
        || a.cache
            .cache_roots
            .iter()
            .any(|root| !root.is_absolute() || !root.join("objects/sha256").is_dir())
    {
        bail!("prepared recovery requires explicit existing stores");
    }
    let profile = Profile {
        config: &a.cache,
        coordination: &Coordination {
            root: a.coordination_root,
        },
        identity: Identity {
            source: Key::parse(&a.source_id)?,
            owner: Key::parse(&a.owner)?,
        },
        expected_revision: a.expected_revision,
        operation: Key::parse(&a.retain_operation)?,
        metadata_root: &a.metadata_root,
        metadata_minimum: &a.metadata_minimum,
        tree_limits: TreeLimits::default(),
    };
    // Check the exact already-owned job tree's input-only shape without
    // reconstructing a compiler plan or asserting its current production basis.
    let mut reader = MetadataReader::open(&a.metadata_root, &a.metadata_minimum)?;
    let entries = Tree::from_root(a.original_input.root.clone())
        .entries(&mut reader, profile.tree_limits)?
        .0;
    let header = get(&entries, "job/identity")?;
    if header.sha256 != a.original_input.context
        || header.bytes > 65536
        || entries
            .keys()
            .any(|name| name.as_str().starts_with("output/"))
    {
        bail!("prepared recovery original job identity differs");
    }
    let header =
        identity::parse_json(&reader.get(header)?.context("original job header missing")?)?;
    if !matches!(
        header["schema"].as_str(),
        Some(
            "tig.native-job-context.experimental-1"
                | "tig.native-job-context.experimental-2"
                | "tig.native-job-context.experimental-3"
        )
    ) || header.get("claim") != Some(&Value::Null)
    {
        bail!("prepared recovery requires an original input job");
    }
    let mut journal = SourceJournal::open(&a.journal_root, &a.journal_minimum)?;
    recover_selection(
        &a.journal_root,
        RecoverySubject {
            job_id: &a.job_id,
            original_input: &a.original_input,
            published: a.published_result.as_ref(),
        },
        &mut journal,
        &profile,
        &a.budget,
    )
}
