//! Durable adoption of one frozen recipe expansion into the native run graph.
//! No provider invocation, scratch allocation, candidate selection or close.
use crate::{
    Tool, admission::Prepared, error, expansion::Frozen, graph, handoff, io_error, retention,
    staging,
};
use reel_workflow::{ErrorKind, Plan, Result, handoff::State};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub runtime: Tool,
    pub target: retention::Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_owner_policy: Option<retention::capacity::Policy>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema: String,
    base_sha256: String,
    frozen_sha256: String,
    config_sha256: String,
    directory_identity: String,
}
#[derive(Debug, Serialize)]
pub struct Summary {
    pub schema: String,
    pub task_id: String,
    pub task_key: String,
    pub admission_identity: String,
    pub state: State,
    pub already_adopted: bool,
}

pub(crate) fn raw(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw =
        serde_json::to_vec(value).map_err(|_| error(ErrorKind::Invalid, "advance-control-json"))?;
    if raw.len() > reel_workflow::MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "advance-control-bytes"));
    }
    Ok(raw)
}
/// Existing controls are immutable. A partial file needs explicit diagnosis,
/// never truncation or an inferred fresh attempt.
pub(crate) fn persist(path: &Path, bytes: &[u8]) -> Result<()> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| error(ErrorKind::NeedsRecovery, "advance-write-preserve"))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(io_error(e)),
    }
    let (actual, _held) = staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if actual != bytes {
        return Err(error(ErrorKind::Conflict, "advance-immutable-control"));
    }
    Ok(())
}

pub fn advance(base: &Plan, frozen: &Frozen, config: &Config, directory: &Path) -> Result<Summary> {
    frozen.check(base)?;
    if !matches!(
        (config.schema.as_str(), &config.retained_owner_policy),
        ("reel.workflow-advance-config.v1", None) | ("reel.workflow-advance-config.v2", Some(_))
    ) || config.target.schema != "reel.workflow-retained-target.v1"
        || frozen
            .bindings
            .tasks
            .contains_key(&frozen.expansion.packet.task.id)
    {
        return Err(error(ErrorKind::Invalid, "advance-config"));
    }
    if let Some(policy) = &config.retained_owner_policy {
        policy.validate()?;
    }
    let _runtime = handoff::runtime(&config.runtime)?;
    // Hold the approved parent while creating/opening the capsule; reject native
    // stores, source repositories and parent-job control namespaces first.
    let (_parent, directory) = frozen.output_path(directory)?;
    let lock_key = reel_workflow::digest(directory.to_string_lossy().as_bytes());
    let _writer = reel_assets::admission::lock(
        &frozen
            .admission
            .scope
            .coordination_root
            .join("workflow-advances")
            .join(lock_key),
    )
    .map_err(|_| error(ErrorKind::Busy, "advance-writer-busy"))?;
    let created = match fs::create_dir(&directory) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(io_error(e)),
    };
    let owned = reel_assets::jobs::open_workspace_directory(&directory)
        .map_err(|_| error(ErrorKind::Denied, "advance-directory-namespace"))?;
    let directory = owned.path();
    let intent = Intent {
        schema: "reel.workflow-advance-intent.v1".into(),
        base_sha256: reel_workflow::canonical_digest(base)?,
        frozen_sha256: reel_workflow::canonical_digest(frozen)?,
        config_sha256: reel_workflow::canonical_digest(config)?,
        directory_identity: owned.identity().into(),
    };
    let intent_path = directory.join("intent.json");
    if !created && !intent_path.exists() {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "advance-original-intent-missing",
        ));
    }
    persist(&intent_path, &raw(&intent)?)?;
    persist(&directory.join("frozen.json"), &raw(frozen)?)?;
    persist(&directory.join("config.json"), &raw(config)?)?;
    let already_adopted =
        directory.join("plan.json").exists() && directory.join("bindings.json").exists();
    let prepared = Prepared::open(
        &frozen.expansion.packet,
        &frozen.source,
        &frozen.admission,
        &frozen.attempt_id,
    )?;
    prepared.check_retained_target(&config.target)?;
    if let Some(policy) = &config.retained_owner_policy {
        prepared.check_retained_owner_policy(&config.target, policy)?;
    }
    let receipt_path = directory.join("admission.json");
    let receipt = if receipt_path.exists() {
        let (bytes, _held) =
            staging::read_held(&receipt_path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
        let receipt = reel_workflow::decode(&bytes)?;
        drop(
            Prepared::open(
                &frozen.expansion.packet,
                &frozen.source,
                &frozen.admission,
                &frozen.attempt_id,
            )?
            .reopen(&receipt)?,
        );
        receipt
    } else if directory.join("profile.json").exists() {
        // Prefer the original durable witness over a newer recovery checkpoint.
        // The reconstructed profile below must still match these exact bytes.
        let (bytes, _held) = staging::read_held(
            &directory.join("profile.json"),
            reel_workflow::MAX_CONTROL_BYTES as u64,
        )?;
        let original: handoff::Profile = reel_workflow::decode(&bytes)?;
        persist(
            &receipt_path,
            &original.binding.admission.canonical_bytes()?,
        )?;
        original.binding.admission
    } else {
        // Missing is a typed exact-operation lookup, not a stderr heuristic.
        // Interrupted/unsettled work is not automatically restarted or replaced.
        let receipt = match prepared.recover() {
            Ok(receipt) => receipt,
            Err(e)
                if e.kind == ErrorKind::Missing && e.code == "admission-original-work-missing" =>
            {
                frozen.accept(base)?
            }
            Err(e) => return Err(e),
        };
        persist(&receipt_path, &receipt.canonical_bytes()?)?;
        receipt
    };
    drop(prepared.reopen(&receipt)?);
    let acknowledgements = directory.join("acknowledgements");
    match fs::create_dir(&acknowledgements) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(io_error(e)),
    }
    let profile = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source: reel_workflow::decode(&raw(&frozen.source)?)?,
        admission: frozen.admission.clone(),
        binding: handoff::Binding {
            schema: if config.retained_owner_policy.is_some() {
                "reel.workflow-native-handoff-binding.v2"
            } else {
                "reel.workflow-native-handoff-binding.v1"
            }
            .into(),
            retained_owner_policy: config.retained_owner_policy.clone(),
            admission: receipt,
            target: config.target.clone(),
        },
        runtime: config.runtime.clone(),
        acknowledgements,
    };
    let profile_path = directory.join("profile.json");
    let profile_raw = raw(&profile)?;
    persist(&profile_path, &profile_raw)?;
    let journal = directory.join("journal");
    if !journal.exists() {
        handoff::create(&journal, &frozen.expansion.packet, &profile)?;
    }
    let observed = handoff::observe(&journal, &profile)?;
    if observed.ticket.packet != frozen.expansion.packet
        || !matches!(observed.state, State::Waiting | State::Retained)
    {
        return Err(error(ErrorKind::Conflict, "advance-original-journal-state"));
    }
    handoff::verify_observation(&journal, &profile, &observed)?;
    let mut bindings: graph::Bindings = reel_workflow::decode(&raw(&frozen.bindings)?)?;
    bindings.tasks.insert(
        frozen.expansion.packet.task.id.clone(),
        graph::Binding {
            journal,
            profile: graph::PinnedFile {
                path: profile_path,
                sha256: reel_workflow::digest(&profile_raw),
            },
            staging_ticket: None,
        },
    );
    persist(&directory.join("plan.json"), &raw(&frozen.expansion.plan)?)?;
    persist(&directory.join("bindings.json"), &raw(&bindings)?)?;
    Ok(Summary {
        schema: "reel.workflow-advance-summary.v1".into(),
        task_id: frozen.expansion.packet.task.id.clone(),
        task_key: frozen.expansion.packet.task_key.clone(),
        admission_identity: profile.binding.admission.operation_identity()?,
        state: observed.state,
        already_adopted,
    })
}
