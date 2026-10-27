//! Explicit frozen native jobs through the existing bounded child runner.
use crate::{
    media_runtime::{Bindings, BoundPrograms},
    tig_inputs::ArchiveProfile,
    tig_job_runner,
    tig_scene::{self, Request},
    tig_scene_production::{
        self,
        jobs::{Coordination, Identity, JobPlan},
    },
};
use anyhow::{Result, bail};
use reel_assets::{Config, jobs};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tig_core::{Digest, LogicalPath, Object, identity, tree::TreeLimits};
use tig_store::{
    Key,
    metadata::{Checkpoint, MetadataReader, MetadataStore},
    source_claims::{self, Selection, SourceJournal},
};

const PROFILE_SCHEMA: &str = "reel.tig-native-job-profile.experimental-1";
const TICKET_SCHEMA: &str = "reel.tig-native-worker.experimental-1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Worker {
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    source: Value,
    context: Selection,
    request: Request,
    runtime: Bindings,
    outputs: BTreeMap<String, u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    schema: String,
    admission: Worker,
    recipe: Digest,
    producer: Object,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    worker: Worker,
    cache: Config,
    journal_root: PathBuf,
    journal_minimum: source_claims::Checkpoint,
    source_id: String,
    owner: String,
    expected_revision: u64,
    start_operation: String,
    retain_operation: String,
    budget_bytes: u64,
    scratch_bytes: u64,
    timeout_seconds: u64,
}
fn read<T: DeserializeOwned>(path: &Path, max: usize, expected: Option<&Object>) -> Result<T> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        bail!("native job intake exceeds budget");
    }
    if expected.is_some_and(|pin| *pin != Object::from_bytes(&bytes)) {
        bail!("native worker admission bytes differ");
    }
    Ok(serde_json::from_value(identity::parse_json(&bytes)?)?)
}
fn pin(programs: &BoundPrograms) -> Result<Object> {
    Ok(Object {
        sha256: Digest::parse(&programs.identity().sha256)?,
        bytes: programs.identity().bytes,
    })
}
fn restore(
    worker: &Worker,
) -> Result<(
    crate::tig_inputs::Source,
    tig_git::source::native::context::ProductionContext,
)> {
    let profile: ArchiveProfile = serde_json::from_value(worker.source.clone())?;
    if !worker.metadata_root.is_absolute() {
        bail!("existing absolute metadata root required");
    }
    let mut reader = MetadataReader::open(&worker.metadata_root, &worker.metadata_minimum)?;
    tig_scene::restore_context(
        profile,
        &worker.request,
        &worker.context,
        &mut reader,
        TreeLimits::default(),
    )
}
fn output_names(worker: &Worker) -> Result<BTreeSet<String>> {
    if worker.outputs.is_empty() || worker.outputs.len() > 62 {
        bail!("native output set exceeds budget");
    }
    for (name, bytes) in &worker.outputs {
        LogicalPath::parse(name)?;
        if *bytes == 0 || name == "worker.json" {
            bail!("invalid native output budget or reserved name");
        }
    }
    for name in [
        "picture.mkv",
        "D.wav",
        "M.wav",
        "E.wav",
        "mix.wav",
        "master.mkv",
        "review.mp4",
        "receipt.json",
    ] {
        if !worker.outputs.contains_key(name) {
            bail!("native output set omits required output");
        }
    }
    Ok(worker.outputs.keys().cloned().collect())
}
/// No initialization or discovery: roots and trusted minimum checkpoints exist.
pub fn run(path: &Path) -> Result<Value> {
    let profile: Profile = read(path, 128 * 1024, None)?;
    if profile.schema != PROFILE_SCHEMA {
        bail!("unsupported native job profile");
    }
    output_names(&profile.worker)?;
    let coordination = Coordination {
        root: profile
            .cache
            .coordination_root
            .clone()
            .ok_or_else(|| anyhow::anyhow!("existing shared coordination root required"))?,
    };
    if !coordination.root.is_absolute() || !coordination.root.is_dir() {
        bail!("existing shared coordination root required");
    }
    if profile.cache.cache_roots.is_empty()
        || profile
            .cache
            .cache_roots
            .iter()
            .any(|p| !p.is_absolute() || !p.join("objects/sha256").is_dir())
    {
        bail!("existing active CAS roots required");
    }
    let identity = Identity {
        source: Key::parse(&profile.source_id)?,
        owner: Key::parse(&profile.owner)?,
    };
    let start = Key::parse(&profile.start_operation)?;
    let retain = Key::parse(&profile.retain_operation)?;
    if !profile.journal_root.is_absolute() {
        bail!("existing absolute journal root required");
    }
    let mut journal = SourceJournal::open(&profile.journal_root, &profile.journal_minimum)?;
    if let Some(state) = journal.source(&identity.source)? {
        if state.owner != identity.owner
            || state.revision != profile.expected_revision
            || state.pending.is_some()
        {
            bail!("native job owner/revision/pending state differs");
        }
    } else if profile.expected_revision != 0 {
        bail!("native job source missing");
    }
    let programs = BoundPrograms::bind(&profile.worker.runtime)?;
    let (mut source, context) = restore(&profile.worker)?;
    let plan = JobPlan::from_context(&mut source, &profile.worker.request, &context, &programs)?;
    let ticket = serde_json::to_string(
        &json!({"schema":TICKET_SCHEMA,"admission":profile.worker,"recipe":plan.recipe().fingerprint(),"producer":pin(&programs)?}),
    )?;
    if ticket.len() > 65_536 {
        bail!("native worker admission exceeds budget");
    }
    let ticket_pin = Object::from_bytes(ticket.as_bytes());
    let outputs: BTreeMap<_, _> = profile
        .worker
        .outputs
        .iter()
        .map(|(name, max)| {
            (
                format!("native/{name}"),
                json!({"path":format!("native/{name}"),"max_bytes":max}),
            )
        })
        .chain(std::iter::once((
            "worker.json".into(),
            json!({"path":"worker.json","max_bytes":65_536}),
        )))
        .collect();
    let request = serde_json::to_vec(&json!({
        "budget_bytes":profile.budget_bytes,"scratch_bytes":profile.scratch_bytes,
        "inputs":{},"tools":{},"installed_tools":{"worker":{"path":std::env::current_exe()?.canonicalize()?,"sha256":profile.worker.runtime.host.sha256,"bytes":profile.worker.runtime.host.bytes}},
        "files":{"worker-input.json":ticket},"prepare_output_parents":false,
        "steps":[{"tool":"worker","args":["worker","worker-input.json","--sha256",ticket_pin.sha256,"--bytes",ticket_pin.bytes.to_string()],"timeout_seconds":profile.timeout_seconds}],"outputs":outputs
    }))?;
    jobs::check_request(&request)
        .map_err(|e| anyhow::anyhow!("native job request refused: {e}"))?;
    // Validation above precedes all metadata and source-journal transitions.
    let mut store = MetadataStore::open(
        &profile.worker.metadata_root,
        &profile.worker.metadata_minimum,
    )?;
    let frozen = plan.freeze(&mut store, TreeLimits::default())?;
    let minimum = store.finish()?;
    drop(store);
    let mut reader = MetadataReader::open(&profile.worker.metadata_root, &minimum)?;
    let ready = plan.readback(frozen.selection(), &mut reader, TreeLimits::default())?;
    let resources = plan.readback(frozen.selection(), &mut reader, TreeLimits::default())?;
    let protected = ready.protect(
        &mut journal,
        &coordination,
        identity.clone(),
        profile.expected_revision,
        &start,
    )?;
    let revision = protected.revision();
    drop(protected);
    let mut manifest = tempfile::Builder::new()
        .prefix("tig-native-request-")
        .tempfile_in(&coordination.root)?;
    manifest.write_all(&request)?;
    manifest.as_file().sync_all()?;
    let result = tig_job_runner::run(
        &coordination.root,
        manifest.path(),
        resources.resources(),
        &mut journal,
        &tig_job_runner::Profile {
            config: &profile.cache,
            coordination: &coordination,
            identity,
            expected_revision: revision,
            operation: retain,
            metadata_root: &profile.worker.metadata_root,
            metadata_minimum: &minimum,
            tree_limits: TreeLimits::default(),
        },
    )?;
    Ok(
        json!({"schema":"reel.tig-native-job-result.experimental-1","result":result,"journal_checkpoint":journal.checkpoint(),"production_claim":null,"complete_claim_coverage":false}),
    )
}
/// Internal trusted child. Coordinator supplies containment and durable claims.
/// Calling this directly does not perform admission or establish retention.
pub fn worker(path: &Path, expected: &Object) -> Result<Value> {
    let ticket: Ticket = read(path, 65_536, Some(expected))?;
    if ticket.schema != TICKET_SCHEMA {
        bail!("unsupported native worker admission");
    }
    let expected = output_names(&ticket.admission)?;
    let programs = BoundPrograms::bind(&ticket.admission.runtime)?;
    if pin(&programs)? != ticket.producer {
        bail!("native worker runtime differs");
    }
    let (mut source, context) = restore(&ticket.admission)?;
    let plan = JobPlan::from_context(&mut source, &ticket.admission.request, &context, &programs)?;
    if plan.recipe().fingerprint() != &ticket.recipe {
        bail!("native worker recipe differs");
    }
    let candidate = tig_scene_production::render_with_programs(
        &mut source,
        &ticket.admission.request,
        &context,
        &programs,
        Path::new("compiled"),
        Path::new("native"),
    )?;
    let actual: BTreeSet<_> = candidate
        .receipt()
        .outputs
        .keys()
        .cloned()
        .chain(std::iter::once("receipt.json".into()))
        .collect();
    if actual != expected {
        bail!("native worker output set differs from admission");
    }
    for (name, max) in &ticket.admission.outputs {
        if fs::metadata(Path::new("native").join(name))?.len() > *max {
            bail!("native worker output exceeds declared size");
        }
    }
    let result = json!({"schema":"reel.tig-native-worker-result.experimental-1","context":ticket.admission.context,"recipe":candidate.recipe().fingerprint(),"producer":ticket.producer,"receipt":candidate.receipt_object(),"production_claim":null});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open("worker.json")?;
    file.write_all(&serde_json::to_vec(&result)?)?;
    file.sync_all()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_worker_ticket_refuses_before_runtime_or_metadata_lookup() {
        let root = tempfile::tempdir().unwrap();
        let ticket = root.path().join("ticket.json");
        fs::write(&ticket, b"{}").unwrap();
        let expected = Object::from_bytes(b"original private coordinator ticket");
        let error = worker(&ticket, &expected).unwrap_err().to_string();
        assert!(error.contains("admission bytes differ"), "{error}");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
