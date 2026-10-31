//! Explicit replay of an already enrolled generic job; no setup or source adoption.
use crate::{
    scene_delivery::FileRef,
    tig_job_runner,
    tig_scene_production::jobs::{Coordination, Identity},
};
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
use tig_assets::graph::{self, recipe};
use tig_core::{Digest, LogicalPath, Object, identity, tree::TreeLimits};
use tig_git::source::native::{AssetSnapshot, job};
use tig_store::{
    Key,
    metadata::{Checkpoint, MetadataReader},
    source_claims::{self, Selection, SourceJournal},
};

pub const SCHEMA: &str = "reel.tig-frozen-job-profile.experimental-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Consumption {
    definition_id: String,
    definition: Object,
    fields: BTreeSet<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    project: PathBuf,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    source_context: Selection,
    source_checkpoint: Digest,
    profiles: Vec<graph::Profile>,
    consumption: Vec<Consumption>,
    operation: recipe::Operation,
    runtime: BTreeMap<LogicalPath, Object>,
    job_context: Selection,
    manifest: FileRef,
    cache: reel_assets::Config,
    journal_root: PathBuf,
    journal_minimum: source_claims::Checkpoint,
    source_id: String,
    owner: String,
    expected_revision: u64,
    retain_operation: String,
}
fn read(path: &Path, maximum: u64) -> Result<(File, Vec<u8>)> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > maximum {
        bail!("frozen job input is not a bounded regular file");
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            bail!("frozen job reparse input refused");
        }
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(path)?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        bail!("frozen job input exceeds bound");
    }
    Ok((file, bytes))
}
fn execute(path: &Path) -> Result<Value> {
    let (_profile_guard, raw) = read(path, 128 * 1024)?;
    let p: Profile = serde_json::from_value(identity::parse_json(&raw)?)?;
    let coordination = p
        .cache
        .coordination_root
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("explicit coordination required"))?;
    if p.schema != SCHEMA
        || p.profiles.is_empty()
        || p.profiles.len() > 128
        || p.consumption.len() > 128
        || p.runtime.len() > 64
        || p.cache.cache_roots.is_empty()
        || [&p.project, &p.metadata_root, &p.journal_root, coordination]
            .iter()
            .any(|root| !root.is_absolute() || !root.is_dir())
        || p.cache
            .cache_roots
            .iter()
            .any(|root| !root.is_absolute() || !root.join("objects/sha256").is_dir())
        || !p.manifest.path.is_absolute()
    {
        bail!("frozen job requires bounded admission and existing absolute stores");
    }
    let identity = Identity {
        source: Key::parse(&p.source_id)?,
        owner: Key::parse(&p.owner)?,
    };
    let operation = Key::parse(&p.retain_operation)?;
    let (_manifest_guard, request) = read(&p.manifest.path, 256 * 1024)?;
    let request_pin = Object::from_bytes(&request);
    if request_pin
        != (Object {
            sha256: Digest::parse(&p.manifest.sha256)?,
            bytes: p.manifest.bytes,
        })
        || p.runtime.get(&LogicalPath::parse("request")?) != Some(&request_pin)
    {
        bail!("frozen job request differs from explicit runtime admission");
    }
    reel_assets::jobs::check_request(&request).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut reader = MetadataReader::open(&p.metadata_root, &p.metadata_minimum)?;
    let limits = graph::Limits {
        nodes: 4096,
        edges: 16384,
        depth: 64,
        bytes: 16 * 1024 * 1024,
    };
    let trees = TreeLimits::default();
    let (mut source, context) = AssetSnapshot::from_production_context(
        &p.source_context,
        &p.profiles,
        &mut reader,
        Default::default(),
        trees,
        limits,
    )?;
    if source.checkpoint() != &p.source_checkpoint
        || Object::from_bytes(context.descriptor()) != p.operation.parameters
    {
        bail!("frozen job source or descriptor differs from admission");
    }
    let query = graph::Query {
        checkpoint: p.source_checkpoint,
        roots: context.roots().clone(),
        kinds: BTreeSet::from([graph::EdgeKind::Production]),
        limits,
    };
    let resolved = graph::resolve(&mut source, &query, &p.profiles)?;
    let consumption: Vec<_> = p
        .consumption
        .into_iter()
        .map(|c| recipe::Consumption {
            definition_id: c.definition_id,
            definition: c.definition,
            fields: c.fields,
        })
        .collect();
    let plan = recipe::plan(
        &resolved,
        &mut source,
        &p.profiles,
        &consumption,
        &p.operation,
        recipe::Limits {
            nodes: limits.nodes,
            edges: limits.edges,
            bytes: limits.bytes,
        },
    )?;
    let resources = job::restore(
        &p.job_context,
        &job::Admission {
            context: &context,
            plan: &plan,
            runtime: &p.runtime,
            output: None,
        },
        &mut reader,
        trees,
    )?;
    let mut journal = SourceJournal::open(&p.journal_root, &p.journal_minimum)?;
    let profile = tig_job_runner::Profile {
        config: &p.cache,
        coordination: &Coordination {
            root: coordination.clone(),
        },
        identity,
        expected_revision: p.expected_revision,
        operation,
        metadata_root: &p.metadata_root,
        metadata_minimum: &p.metadata_minimum,
        tree_limits: trees,
    };
    let mut result = tig_job_runner::run(
        &p.project,
        &p.manifest.path,
        &resources,
        &mut journal,
        &profile,
    )?;
    // The generic runner returns Err for a failed child, even when its failed
    // result was successfully retained. Only this successful return is true.
    result["succeeded"] = Value::Bool(true);
    result["production_claim"] = Value::Null;
    result["creative_approval"] = Value::Bool(false);
    Ok(result)
}
/// Trusted caller admission, existing metadata and exact owner required. Public
/// failures omit private paths/IDs/text; successful runner output stays private.
pub fn run(path: &Path) -> Result<Value> {
    execute(path).map_err(|_| {
        anyhow::anyhow!(
            "frozen job refused or incomplete; preserve owned scratch and inspect private state"
        )
    })
}
