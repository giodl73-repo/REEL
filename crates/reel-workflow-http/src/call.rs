//! Explicit Windows-owned transport evidence. Native protection/retention is a
//! separate caller responsibility; HTTP completion is never task completion.
use crate::{Config, Prepared, error, proxy};
use reel_assets::{
    admission as capacity,
    jobs::{WorkspaceDirectory, open_workspace_directory},
};
use reel_workflow::{ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Payload {
    Json {
        operation_id: String,
        body: Input,
    },
    Proxy {
        spec: Box<proxy::Spec>,
        inputs: BTreeMap<String, Input>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub parent: PathBuf,
    pub maximum_bytes: u64,
    pub capacity_policy: capacity::Policy,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub http: Config,
    pub payload: Payload,
    /// Existing private coordination directory: reuse it for this operation.
    pub registry: PathBuf,
    pub workspace: Workspace,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    schema: String,
    operation_id: String,
    request_sha256: String,
    registry_identity: String,
    parent_identity: String,
    root_identity: String,
    reservation: String,
    volume: String,
    maximum_bytes: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Complete {
    schema: String,
    operation_id: String,
    request_sha256: String,
    http_config_sha256: String,
    body_sha256: String,
    head_sha256: String,
    status: u16,
    response_bytes: u64,
    response_sha256: String,
}
fn scoped(_: impl std::fmt::Display) -> reel_workflow::Error {
    error(ErrorKind::NeedsRecovery, "http-owned-state-preserve")
}
fn capacity_error(value: Box<dyn std::error::Error>) -> reel_workflow::Error {
    let kind = if value.downcast_ref::<capacity::MetadataBusy>().is_some() {
        ErrorKind::Busy
    } else if value.downcast_ref::<capacity::AdmissionDenied>().is_some() {
        ErrorKind::Budget
    } else {
        ErrorKind::NeedsRecovery
    };
    error(kind, "http-capacity-state-preserve")
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw =
        serde_json::to_vec(value).map_err(|_| error(ErrorKind::Invalid, "http-control-json"))?;
    if raw.len() > reel_workflow::MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "http-control-bytes"));
    }
    Ok(raw)
}
fn ordinary(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.is_file() && metadata.file_attributes() & 0x400 == 0
}
fn read(path: &Path, maximum: u64) -> Result<(Vec<u8>, File)> {
    let _parent = open_workspace_directory(
        path.parent()
            .ok_or_else(|| error(ErrorKind::Invalid, "http-file-parent"))?,
    )
    .map_err(scoped)?;
    use std::os::windows::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x0020_0000)
        .open(path)
        .map_err(scoped)?;
    let metadata = file.metadata().map_err(scoped)?;
    if !ordinary(&metadata) {
        return Err(error(ErrorKind::Denied, "http-ordinary-file-required"));
    }
    if metadata.len() > maximum {
        return Err(error(ErrorKind::Budget, "http-file-bytes"));
    }
    let mut raw = Vec::new();
    Read::by_ref(&mut file)
        .take(maximum + 1)
        .read_to_end(&mut raw)
        .map_err(scoped)?;
    if raw.len() as u64 > maximum {
        return Err(error(ErrorKind::Budget, "http-file-growth"));
    }
    Ok((raw, file))
}
fn pinned(input: &Input, maximum: u64) -> Result<(Vec<u8>, File)> {
    if input.bytes > maximum {
        return Err(error(ErrorKind::Budget, "http-input-bytes"));
    }
    let (raw, file) = read(&input.path, input.bytes)?;
    if raw.len() as u64 != input.bytes || reel_workflow::digest(&raw) != input.sha256 {
        return Err(error(ErrorKind::Integrity, "http-input-pin"));
    }
    Ok((raw, file))
}
fn write(file: &mut File, raw: &[u8]) -> Result<()> {
    file.write_all(raw)
        .and_then(|_| file.sync_all())
        .map_err(scoped)
}
fn create(path: &Path, raw: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(scoped)?;
    write(&mut file, raw)
}
fn preserve(path: &Path, raw: &[u8]) -> Result<()> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => write(&mut file, raw)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(scoped(e)),
    }
    let (observed, _guard) = read(path, raw.len() as u64)?;
    if observed != raw {
        return Err(error(ErrorKind::Conflict, "http-original-evidence-changed"));
    }
    Ok(())
}
impl Request {
    fn operation(&self) -> Result<String> {
        if self.schema != "reel.workflow-http-call.v1" {
            return Err(error(ErrorKind::Invalid, "http-call-schema"));
        }
        match &self.payload {
            Payload::Json { operation_id, .. } if crate::token(operation_id) => {
                Ok(operation_id.clone())
            }
            Payload::Proxy { spec, .. } => spec.operation_id(),
            _ => Err(error(ErrorKind::Invalid, "http-operation-identity")),
        }
    }
    fn body(&self) -> Result<(Vec<u8>, Vec<File>)> {
        let maximum = self.http.request_byte_limit.min(crate::MAX_BYTES);
        match &self.payload {
            Payload::Json { body, .. } => {
                let (raw, file) = pinned(body, maximum)?;
                Ok((raw, vec![file]))
            }
            Payload::Proxy { spec, inputs } => {
                if inputs.len() > 128 {
                    return Err(error(ErrorKind::Budget, "proxy-input-count"));
                }
                // Bound aggregate admission before allocating or reading inputs.
                let total = inputs
                    .values()
                    .try_fold(0u64, |n, i| n.checked_add(i.bytes))
                    .ok_or_else(|| error(ErrorKind::Budget, "proxy-input-bytes"))?;
                if total > maximum {
                    return Err(error(ErrorKind::Budget, "proxy-input-bytes"));
                }
                let mut values = BTreeMap::new();
                let mut held = Vec::new();
                for (id, input) in inputs {
                    let asset = spec
                        .packet
                        .task
                        .inputs
                        .iter()
                        .find(|a| &a.logical_id == id)
                        .ok_or_else(|| error(ErrorKind::Denied, "proxy-undeclared-input"))?;
                    if asset.bytes != input.bytes || asset.sha256 != input.sha256 {
                        return Err(error(ErrorKind::Conflict, "proxy-input-binding"));
                    }
                    let (raw, file) = pinned(input, maximum)?;
                    values.insert(id.clone(), raw);
                    held.push(file);
                }
                Ok((spec.compose(&values)?, held))
            }
        }
    }
    fn config(&self, parent: &Path, registry: &Path) -> Result<reel_assets::Config> {
        self.workspace
            .capacity_policy
            .validate()
            .map_err(|_| error(ErrorKind::Invalid, "http-capacity-policy"))?;
        if !(8 * 1024 * 1024..=1024 * 1024 * 1024).contains(&self.workspace.maximum_bytes)
            || self.workspace.maximum_bytes > self.workspace.capacity_policy.max_object_bytes
        {
            return Err(error(ErrorKind::Budget, "http-workspace-budget"));
        }
        let mut policy = self.workspace.capacity_policy.clone();
        if let Some(minimum) = policy.tier_minimum_free_bytes.get(&self.workspace.parent) {
            policy.minimum_free_bytes = policy.minimum_free_bytes.max(*minimum);
        }
        Ok(reel_assets::Config {
            cache_roots: vec![parent.into()],
            coordination_root: Some(registry.into()),
            capacity: policy,
            ..Default::default()
        })
    }
}
struct Owned {
    _registry: WorkspaceDirectory,
    _calls: WorkspaceDirectory,
    _parent: WorkspaceDirectory,
    root: WorkspaceDirectory,
    _lease: capacity::Reservation,
    _controls: Vec<File>,
}
fn preparation(
    pin: &str,
    operation: &str,
    registry: &WorkspaceDirectory,
    parent: &WorkspaceDirectory,
) -> Result<Vec<u8>> {
    encode(
        &serde_json::json!({"schema":"reel.workflow-http-preparation.v1","request_sha256":pin,"operation_id":operation,"registry_identity":registry.identity(),"parent_identity":parent.identity()}),
    )
}
fn reservation_record(
    pin: &str,
    operation: &str,
    reservation: &str,
    volume: &str,
    maximum_bytes: u64,
) -> Result<Vec<u8>> {
    encode(
        &serde_json::json!({"schema":"reel.workflow-http-capacity.v1","request_sha256":pin,"operation_id":operation,"reservation":reservation,"volume":volume,"maximum_bytes":maximum_bytes}),
    )
}
fn open(request: &Request, pin: &str, operation: &str) -> Result<Owned> {
    let registry = open_workspace_directory(&request.registry).map_err(scoped)?;
    let calls = open_workspace_directory(&registry.path().join("calls")).map_err(scoped)?;
    let parent = open_workspace_directory(&request.workspace.parent).map_err(scoped)?;
    let (claim, claim_file) = read(
        &calls.path().join(format!("{operation}.prepare.json")),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    if claim != preparation(pin, operation, &registry, &parent)? {
        return Err(error(
            ErrorKind::Conflict,
            "http-original-preparation-binding",
        ));
    }
    let root = open_workspace_directory(&parent.path().join(format!("http-{operation}")))
        .map_err(scoped)?;
    let (raw, owner_file) = read(
        &calls.path().join(format!("{operation}.json")),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    let owner: Owner = reel_workflow::decode(&raw)?;
    let (capacity_record, capacity_file) = read(
        &calls.path().join(format!("{operation}.capacity.json")),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    if capacity_record
        != reservation_record(
            pin,
            operation,
            &owner.reservation,
            &owner.volume,
            owner.maximum_bytes,
        )?
    {
        return Err(error(ErrorKind::Conflict, "http-original-capacity-binding"));
    }
    if owner.schema != "reel.workflow-http-owner.v1"
        || owner.operation_id != operation
        || owner.request_sha256 != pin
        || owner.registry_identity != registry.identity()
        || owner.parent_identity != parent.identity()
        || owner.root_identity != root.identity()
        || owner.maximum_bytes != request.workspace.maximum_bytes
    {
        return Err(error(ErrorKind::Conflict, "http-original-owner-binding"));
    }
    let (intent, intent_file) = read(
        &root.path().join("invocation.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    if reel_workflow::digest(&intent) != pin {
        return Err(error(ErrorKind::Integrity, "http-original-invocation"));
    }
    let lease = capacity::resume_preserved_on(
        &request.config(parent.path(), registry.path())?,
        parent.path(),
        &owner.reservation,
        &owner.volume,
        owner.maximum_bytes,
    )
    .map_err(capacity_error)?;
    Ok(Owned {
        _registry: registry,
        _calls: calls,
        _parent: parent,
        root,
        _lease: lease,
        _controls: vec![claim_file, capacity_file, owner_file, intent_file],
    })
}
fn prepare(request: &Request, raw: &[u8], pin: &str, operation: &str, body: &[u8]) -> Result<()> {
    let registry = open_workspace_directory(&request.registry).map_err(scoped)?;
    let parent = open_workspace_directory(&request.workspace.parent).map_err(scoped)?;
    if parent.path().starts_with(registry.path()) || registry.path().starts_with(parent.path()) {
        return Err(error(ErrorKind::Denied, "http-workspace-registry-overlap"));
    }
    match fs::create_dir(registry.path().join("calls")) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(scoped(e)),
    }
    let calls = open_workspace_directory(&registry.path().join("calls")).map_err(scoped)?;
    let owner_path = calls.path().join(format!("{operation}.json"));
    let claim_path = calls.path().join(format!("{operation}.prepare.json"));
    let root_path = parent.path().join(format!("http-{operation}"));
    if fs::symlink_metadata(&owner_path).is_ok()
        || fs::symlink_metadata(&root_path).is_ok()
        || fs::symlink_metadata(&claim_path).is_ok()
    {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "http-original-operation-exists",
        ));
    }
    let required = (body.len() as u64)
        .checked_add(request.http.response_byte_limit.saturating_mul(3))
        .and_then(|n| n.checked_add(8 * 1024 * 1024))
        .ok_or_else(|| error(ErrorKind::Budget, "http-workspace-budget"))?;
    if required > request.workspace.maximum_bytes {
        return Err(error(ErrorKind::Budget, "http-workspace-budget"));
    }
    let config = request.config(parent.path(), registry.path())?;
    // Fence this operation BEFORE admission can allocate a durable reservation.
    // A partial/failed preparation must not allocate another one on retry.
    create(
        &claim_path,
        &preparation(pin, operation, &registry, &parent)?,
    )?;
    let lease = capacity::reserve_on(
        parent.path(),
        &config,
        parent.path(),
        request.workspace.maximum_bytes,
    )
    .map_err(capacity_error)?;
    let reservation = lease.id.clone();
    let volume = lease.volume_id.clone();
    lease.retain_for_recovery();
    create(
        &calls.path().join(format!("{operation}.capacity.json")),
        &reservation_record(
            pin,
            operation,
            &reservation,
            &volume,
            request.workspace.maximum_bytes,
        )?,
    )?;
    fs::create_dir(&root_path).map_err(scoped)?;
    let root = open_workspace_directory(&root_path).map_err(scoped)?;
    create(&root.path().join("invocation.json"), raw)?;
    create(&root.path().join("request.json"), body)?;
    create(
        &owner_path,
        &encode(&Owner {
            schema: "reel.workflow-http-owner.v1".into(),
            operation_id: operation.into(),
            request_sha256: pin.into(),
            registry_identity: registry.identity().into(),
            parent_identity: parent.identity().into(),
            root_identity: root.identity().into(),
            reservation,
            volume,
            maximum_bytes: request.workspace.maximum_bytes,
        })?,
    )
}
fn send(
    prepared: Prepared,
    owned: &Owned,
    pin: &str,
    http: &Config,
    operation: &str,
    body: &[u8],
) -> Result<()> {
    let root = owned.root.path();
    let mut response = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("response.bin"))
        .map_err(scoped)?;
    let reply = prepared.send_observed(
        |intent| create(&root.join("call-intent.json"), &encode(intent)?),
        |head| create(&root.join("http-head.json"), &encode(head)?),
        |bytes| write(&mut response, bytes),
    )?;
    drop(response);
    let (captured, _guard) = read(&root.join("response.bin"), http.response_byte_limit)?;
    if reel_workflow::digest(&captured) != reply.body_sha256 || captured.len() != reply.body.len() {
        return Err(error(ErrorKind::Integrity, "http-response-readback"));
    }
    let (head, _head) = read(
        &root.join("http-head.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    create(
        &root.join("response-complete.json"),
        &encode(&Complete {
            schema: "reel.workflow-http-complete.v1".into(),
            operation_id: operation.into(),
            request_sha256: pin.into(),
            http_config_sha256: reel_workflow::canonical_digest(http)?,
            body_sha256: reel_workflow::digest(body),
            head_sha256: reel_workflow::digest(&head),
            status: reply.status,
            response_bytes: captured.len() as u64,
            response_sha256: reply.body_sha256,
        })?,
    )
}
fn finish(
    request: &Request,
    pin: &str,
    operation: &str,
    owned: &Owned,
) -> Result<serde_json::Value> {
    let root = owned.root.path();
    let (raw, _complete) = read(
        &root.join("response-complete.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )
    .map_err(|_| error(ErrorKind::NeedsRecovery, "http-original-outcome-uncertain"))?;
    let complete: Complete = reel_workflow::decode(&raw)?;
    let (body, _body) = read(
        &root.join("request.json"),
        request.http.request_byte_limit.min(crate::MAX_BYTES),
    )?;
    let (raw, _intent) = read(
        &root.join("call-intent.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    let intent: serde_json::Value = reel_workflow::decode(&raw)?;
    let (head, _head) = read(
        &root.join("http-head.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    let observed: crate::ResponseHead = reel_workflow::decode(&head)?;
    let (response, _response) = read(
        &root.join("response.bin"),
        request.http.response_byte_limit.min(crate::MAX_BYTES),
    )?;
    let config_pin = reel_workflow::canonical_digest(&request.http)?;
    if complete.schema != "reel.workflow-http-complete.v1"
        || complete.operation_id != operation
        || complete.request_sha256 != pin
        || complete.http_config_sha256 != config_pin
        || complete.body_sha256 != reel_workflow::digest(&body)
        || complete.head_sha256 != reel_workflow::digest(&head)
        || complete.response_bytes != response.len() as u64
        || complete.response_sha256 != reel_workflow::digest(&response)
        || observed.schema != "reel.workflow-http-head.v1"
        || observed.status != complete.status
        || intent["schema"] != "reel.workflow-http-intent.v1"
        || intent["operation_id"] != operation
        || intent["config_sha256"] != config_pin
        || intent["request_sha256"] != complete.body_sha256
        || intent["request_bytes"] != body.len() as u64
    {
        return Err(error(
            ErrorKind::Integrity,
            "http-original-response-binding",
        ));
    }
    if let Payload::Proxy { spec, .. } = &request.payload {
        if !(200..300).contains(&complete.status) {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "proxy-http-status-reconcile",
            ));
        }
        let candidate = spec.normalize(&response)?;
        let outputs = root.join("outputs");
        match fs::create_dir(&outputs) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(scoped(e)),
        }
        let outputs = open_workspace_directory(&outputs).map_err(scoped)?;
        for (name, bytes) in &candidate.files {
            preserve(&outputs.path().join(name), bytes)?;
        }
        for (index, entry) in fs::read_dir(outputs.path()).map_err(scoped)?.enumerate() {
            if index >= 128 {
                return Err(error(ErrorKind::Budget, "proxy-output-directory-bound"));
            }
            let name = entry
                .map_err(scoped)?
                .file_name()
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| error(ErrorKind::Conflict, "proxy-output-directory"))?;
            if !candidate.files.contains_key(&name) {
                return Err(error(ErrorKind::Conflict, "proxy-output-directory"));
            }
        }
        preserve(&root.join("result.json"), &encode(&candidate.result)?)?;
        preserve(
            &root.join("returns.json"),
            &encode(
                &serde_json::json!({"schema":"reel.workflow-returns.v1","root":outputs.path(),"files":candidate.files.keys().map(|n|(n,n)).collect::<BTreeMap<_,_>>()}),
            )?,
        )?;
    }
    Ok(
        serde_json::json!({"ok":true,"scope":"private-http-evidence","status":complete.status,"candidate_ready":matches!(request.payload,Payload::Proxy {..}),"task_completed":false,"native_retained":false,"selected":false,"scratch_retired":false,"capacity_released":false}),
    )
}
/// `resume` only reconciles captured evidence; it never executes HTTP.
pub fn execute(path: &Path, pin: &str, resume: bool) -> Result<serde_json::Value> {
    let (raw, _request) = read(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if reel_workflow::digest(&raw) != pin {
        return Err(error(ErrorKind::Integrity, "http-call-request-pin"));
    }
    let request: Request = reel_workflow::decode(&raw)?;
    let operation = request.operation()?;
    let mut inputs = Vec::new();
    let prepared = if !resume {
        let (body, held) = request.body()?;
        inputs = held;
        let prepared = request
            .http
            .prepare(&operation, &body, |name| std::env::var(name).ok())?;
        prepare(&request, &raw, pin, &operation, &body)?;
        Some((prepared, body))
    } else {
        None
    };
    let owned = open(&request, pin, &operation)?;
    if let Some((prepared, body)) = prepared {
        send(prepared, &owned, pin, &request.http, &operation, &body)?;
    }
    drop(inputs);
    finish(&request, pin, &operation, &owned)
}
