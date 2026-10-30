//! Explicit bounded episode jobs over one frozen source and existing stores.
//! Coordinator planning grants no scene eligibility; the contained worker must
//! obtain fresh owned primary-reference proofs before assembling any movie.
use crate::tig_native_job_cli::metadata_wait::{MetadataWait, Waiting};
use crate::{
    media_runtime::{Bindings, BoundPrograms},
    tig_episode::{self, ProtectionBudget},
    tig_inputs::{ArchiveProfile, Source},
    tig_native_job_cli::preparation::Preparation,
    tig_scene,
    tig_scene_production::jobs::{self, Coordination, Identity},
};
use anyhow::{Context, Result, bail};
use reel_assets::{Config, jobs as runner};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::{context::ProductionContext, job};
use tig_store::{
    Key,
    metadata::{Checkpoint, MetadataReader, MetadataStore},
    source_claims::{self, Command, Selection, SourceJournal},
};
mod handoff;
mod planning;
mod worker;
pub use handoff::finish_episode;
pub use worker::worker;

const SCHEMA: &str = "reel.tig-episode-job.experimental-1";
const TICKET: &str = "reel.tig-episode-worker.experimental-1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Scene {
    id: String,
    request: tig_scene::Request,
    output_id: String,
    output_definition: String,
    #[serde(default)]
    compiled: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Presentation {
    id: String,
    request: crate::tig_presentation::Request,
    output_id: String,
    output_definition: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    source: Value,
    context: Selection,
    request: tig_episode::Request,
    runtime: Bindings,
    scenes: Vec<Scene>,
    presentations: Vec<Presentation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    inputs: Inputs,
    cache: Config,
    preparation_cache: PathBuf,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    journal_root: PathBuf,
    journal_minimum: source_claims::Checkpoint,
    source_id: String,
    owner: String,
    expected_revision: u64,
    operation: String,
    budget_bytes: u64,
    scratch_bytes: u64,
    timeout_seconds: u64,
    reference_staging_bytes: u64,
    reference_picture_processes: u16,
    max_objects: usize,
    max_logical_bytes: u64,
    outputs: BTreeMap<String, u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    schema: String,
    inputs: Inputs,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    journal_root: PathBuf,
    journal_minimum: source_claims::Checkpoint,
    coordination_root: PathBuf,
    source_id: String,
    owner: String,
    operation: String,
    resources: Selection,
    root_revision: u64,
    recipe: Digest,
    reference_staging_bytes: u64,
    reference_picture_processes: u16,
    timeout_seconds: u64,
    max_objects: usize,
    max_logical_bytes: u64,
    outputs: BTreeMap<String, u64>,
}
fn read<T: DeserializeOwned>(path: &Path, limit: usize, expected: Option<&Object>) -> Result<T> {
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > limit || expected.is_some_and(|pin| pin != &Object::from_bytes(&raw)) {
        bail!("episode admission bytes differ or exceed budget");
    }
    Ok(serde_json::from_value(identity::parse_json(&raw)?)?)
}
fn key(value: &str) -> Result<Key> {
    Ok(Key::parse(value)?)
}
fn runtime(programs: &BoundPrograms) -> Result<BTreeMap<LogicalPath, Object>> {
    programs
        .components()
        .map(|(name, pin)| {
            Ok((
                LogicalPath::parse(Digest::blob(name.as_bytes()).as_str())?,
                Object {
                    sha256: Digest::parse(&pin.sha256)?,
                    bytes: pin.bytes,
                },
            ))
        })
        .collect()
}
fn phase(source: &str, operation: &str, label: &str) -> Result<Key> {
    key(Digest::blob(&identity::canonical_json(
        &json!({"schema":TICKET,"source":source,"operation":operation,"phase":label}),
    )?)
    .as_str())
}
fn restore(
    inputs: &Inputs,
    root: &Path,
    minimum: &Checkpoint,
    wait: &MetadataWait,
) -> Result<(Source, ProductionContext)> {
    let mut reader = Waiting::new(wait.run(|| MetadataReader::open(root, minimum))?, wait);
    tig_episode::restore_composition_context(
        serde_json::from_value(inputs.source.clone())?,
        &inputs.request,
        &inputs.context,
        &mut reader,
        TreeLimits::default(),
    )
}
fn protection(max_objects: usize, max_logical_bytes: u64) -> Result<ProtectionBudget> {
    if max_objects == 0
        || max_objects > 1_000_000
        || max_logical_bytes == 0
        || max_logical_bytes > 9_007_199_254_740_991
    {
        bail!("invalid episode protection budget");
    }
    Ok(ProtectionBudget {
        max_objects,
        max_logical_bytes,
    })
}
fn admit(resources: &job::JobContext, budget: &ProtectionBudget) -> Result<()> {
    if resources.objects().len() > budget.max_objects
        || resources.logical_bytes() > budget.max_logical_bytes
    {
        bail!("episode complete input resource budget exceeded");
    }
    Ok(())
}
fn validate(p: &Profile) -> Result<()> {
    let budget = protection(p.max_objects, p.max_logical_bytes)?;
    let _ = budget;
    if p.schema != SCHEMA
        || p.inputs.scenes.len() + p.inputs.presentations.len() > 128
        || p.inputs.scenes.is_empty() && p.inputs.presentations.is_empty()
        || !p.cache.cache_roots.contains(&p.preparation_cache)
        || !p.preparation_cache.is_absolute()
        || !p.preparation_cache.join("objects/sha256").is_dir()
        || !p.metadata_root.is_absolute()
        || !p.journal_root.is_absolute()
        || !(1..=runner::MAX_STEP_TIMEOUT_SECONDS).contains(&p.timeout_seconds)
        || p.reference_staging_bytes == 0
        || p.reference_staging_bytes > p.scratch_bytes
        || !(2..=130).contains(&p.reference_picture_processes)
        || p.outputs.is_empty()
        || p.outputs.len() > 3
        || !p.outputs.contains_key("receipt.json")
        || p.outputs.iter().any(|(name, max)| {
            !matches!(name.as_str(), "master.mkv" | "episode.mp4" | "receipt.json")
                || *max == 0
                || *max > p.budget_bytes
        })
    {
        bail!("episode profile scope, existing roots or budgets invalid");
    }
    key(&p.source_id)?;
    key(&p.owner)?;
    key(&p.operation)?;
    let root = p
        .cache
        .coordination_root
        .as_ref()
        .context("existing episode coordination root required")?;
    if !root.is_absolute()
        || !root.is_dir()
        || p.cache
            .cache_roots
            .iter()
            .any(|root| !root.is_absolute() || !root.join("objects/sha256").is_dir())
    {
        bail!("existing episode roots required");
    }
    Ok(())
}

/// Explicit execution only. Uses existing stores and runner admission; never
/// initializes caches, changes project bindings or silently approves outputs.
pub fn run(path: &Path) -> Result<Value> {
    let p: Profile = read(path, 128 * 1024, None)?;
    validate(&p)?;
    let wait = MetadataWait::default();
    let programs = BoundPrograms::bind(&p.inputs.runtime)?;
    let coordination = Coordination {
        root: p
            .cache
            .coordination_root
            .clone()
            .context("coordination root missing")?,
    };
    let identity = Identity {
        source: key(&p.source_id)?,
        owner: key(&p.owner)?,
    };
    let mut journal = SourceJournal::open(&p.journal_root, &p.journal_minimum)?;
    if let Some(current) = journal.source(&identity.source)? {
        if current.owner != identity.owner
            || current.revision != p.expected_revision
            || current.pending.is_some()
        {
            bail!("episode source owner, revision or pending state differs");
        }
    } else if p.expected_revision != 0 {
        bail!("episode source missing");
    }
    let preparation = Preparation::open_on(
        &p.cache,
        &p.journal_root,
        &p.preparation_cache,
        2 * (p.inputs.scenes.len() + p.inputs.presentations.len()) as u64 + 2,
    )?;
    let (mut source, context) = restore(&p.inputs, &p.metadata_root, &p.metadata_minimum, &wait)?;
    let mut store = Waiting::new(
        wait.run(|| MetadataStore::open(&p.metadata_root, &p.metadata_minimum))?,
        &wait,
    );
    let root = planning::freeze(
        &p.inputs,
        &mut source,
        &context,
        &programs,
        &mut store,
        preparation.path(),
    )?;
    if !p.outputs.keys().eq(root.outputs.iter()) {
        bail!("episode output names differ from source contract");
    }
    admit(
        &root.resources,
        &protection(p.max_objects, p.max_logical_bytes)?,
    )?;
    let minimum = wait.run(|| store.inner.finish())?;
    drop(store);
    let mut reader = Waiting::new(
        wait.run(|| MetadataReader::open(&p.metadata_root, &minimum))?,
        &wait,
    );
    planning::readback(&p.inputs, &source, &context, &programs, &root, &mut reader)?;
    preparation.finish()?;
    let ticket = identity::canonical_json(&json!({"schema":TICKET,"inputs":p.inputs,
        "metadata_root":p.metadata_root,"metadata_minimum":minimum,"journal_root":p.journal_root,
        "journal_minimum":journal.checkpoint(),"coordination_root":coordination.root,
        "source_id":p.source_id,"owner":p.owner,"operation":p.operation,
        "resources":root.resources.selection(),"recipe":root.recipe.fingerprint(),
        "root_revision":p.expected_revision.checked_add(2).context("episode root revision overflow")?,
        "reference_staging_bytes":p.reference_staging_bytes,"reference_picture_processes":p.reference_picture_processes,
        "timeout_seconds":p.timeout_seconds,"max_objects":p.max_objects,"max_logical_bytes":p.max_logical_bytes,"outputs":p.outputs}))?;
    if ticket.len() > 65536 {
        bail!("episode worker ticket exceeds budget");
    }
    let ticket_pin = Object::from_bytes(&ticket);
    let mut outputs = p
        .outputs
        .iter()
        .map(|(name, max)| {
            (
                format!("native/{name}"),
                json!({"path":format!("native/{name}"),"max_bytes":max}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    outputs.insert(
        "worker.json".into(),
        json!({"path":"worker.json","max_bytes":65536}),
    );
    let request = identity::canonical_json(
        &json!({"output_count_limit":16,"budget_bytes":p.budget_bytes,"scratch_bytes":p.scratch_bytes,"step_timeout_limit_seconds":p.timeout_seconds,
        "inputs":{},"tools":{},"installed_tools":{"worker":{"path":std::env::current_exe()?.canonicalize()?,"sha256":p.inputs.runtime.host.sha256,"bytes":p.inputs.runtime.host.bytes}},
        "files":{"episode-ticket.json":std::str::from_utf8(&ticket)?},"prepare_output_parents":false,
        "steps":[{"tool":"worker","args":["episode-worker","episode-ticket.json","--sha256",ticket_pin.sha256,"--bytes",ticket_pin.bytes.to_string()],"timeout_seconds":p.timeout_seconds}],"outputs":outputs}),
    )?;
    runner::check_request(&request)
        .map_err(|error| anyhow::anyhow!("episode runner admission: {error}"))?;
    // Explicit request validation precedes source-journal adoption.
    let _fence = jobs::fence(&coordination, &journal, &identity)?;
    let prepared = journal.apply(
        phase(&p.source_id, &p.operation, "prepare")?,
        Command::Prepare {
            source: identity.source.clone(),
            owner: identity.owner.clone(),
            expected_revision: p.expected_revision,
            selection: root.resources.selection().clone(),
        },
    )?;
    let adopted = journal.apply(
        phase(&p.source_id, &p.operation, "adopt")?,
        Command::Adopt {
            source: identity.source.clone(),
            owner: identity.owner.clone(),
            expected_revision: prepared.source_revision,
            prepared: prepared.reference,
        },
    )?;
    drop(_fence);
    let mut manifest = tempfile::Builder::new()
        .prefix("tig-episode-request-")
        .tempfile_in(&coordination.root)?;
    manifest.write_all(&request)?;
    manifest.as_file_mut().sync_all()?;
    let result = crate::tig_job_runner::run_with_dependencies(
        &p.journal_root,
        manifest.path(),
        &root.resources,
        &mut journal,
        &crate::tig_job_runner::Profile {
            config: &p.cache,
            coordination: &coordination,
            identity,
            expected_revision: adopted.source_revision,
            operation: phase(&p.source_id, &p.operation, "retain")?,
            metadata_root: &p.metadata_root,
            metadata_minimum: &minimum,
            tree_limits: TreeLimits::default(),
        },
    );
    // A contained worker failure still returns an error after its diagnostic
    // result was durably retained. Cleanup follows that authoritative retained
    // outcome, never the presence/absence of a success report or error wording.
    let handoff = handoff::finish(&p, &mut journal).with_context(|| {
        result.as_ref().err().map_or_else(
            || "episode retained-context handoff incomplete".to_owned(),
            |error| {
                format!("episode retained-context handoff incomplete after worker error: {error}")
            },
        )
    })?;
    let succeeded = result.is_ok() && handoff["succeeded"] == true;
    let result = result.unwrap_or_else(|error| json!({"schema":"reel.failed-job-result.v1","error":error.to_string(),"outputs":{},"production_claim":null}));
    Ok(
        json!({"schema":"reel.tig-episode-job-result.experimental-1","succeeded":succeeded,"result":result,"journal_checkpoint":journal.checkpoint(),
        "production_claim":null,"approval":false,"bindings_changed":false,"handoff":handoff,"temporary_owner_cleanup_required":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_episode_ticket_refuses_before_runtime_store_or_media_access() {
        let root = tempfile::tempdir().unwrap();
        let ticket = root.path().join("ticket.json");
        fs::write(&ticket, b"{}").unwrap();
        let expected = Object::from_bytes(b"the admitted episode worker ticket");
        let error = worker(&ticket, &expected).unwrap_err().to_string();
        assert!(error.contains("admission bytes differ"));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
