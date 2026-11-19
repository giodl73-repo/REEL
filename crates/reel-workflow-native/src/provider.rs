//! Protected HTTP proxy calls. Original call uncertainty never launches another.
use super::*;
use crate::{graph, handoff};
use base64::{Engine, engine::general_purpose::STANDARD};
use reel_workflow::{Mode, Operation, Provenance, ReturnedResult, handoff::State};

const REQUEST: &str = "provider-request.json";
const RESPONSE: &str = "provider-response.json";
const CONFIG: &str = "provider-config.json";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub provider: String,
    pub model: String,
    pub send_inputs: Vec<String>,
    pub http: reel_workflow_http::Config,
    pub workspace: Profile,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    schema: String,
    config: Config,
    reservation: Reservation,
    owner: Owner,
    admission: Receipt,
    batch: graph::PinnedFile,
    packet: Packet,
}
impl Ticket {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        json(self)
    }
    pub fn directory(&self) -> PathBuf {
        self.config.workspace.parent.join(&self.owner.name)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Encoded {
    media_type: String,
    base64: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    schema: String,
    operation_id: String,
    provider: String,
    model: String,
    #[serde(deserialize_with = "unique_outputs")]
    outputs: BTreeMap<String, Encoded>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HttpComplete {
    schema: String,
    operation_id: String,
    status: u16,
    bytes: u64,
    sha256: String,
}
fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
pub(crate) fn check(packet: &Packet, c: &Config) -> Result<()> {
    if c.schema != "reel.workflow-provider-proxy.v1"
        || !name(&c.provider)
        || !name(&c.model)
        || packet.task.operation == Operation::DeterministicTool
        || packet.task.tool.is_some()
    {
        return Err(error(ErrorKind::Invalid, "provider-config-operation"));
    }
    if c.send_inputs.len() > 128
        || c.send_inputs.iter().collect::<BTreeSet<_>>().len() != c.send_inputs.len()
        || c.send_inputs
            .iter()
            .any(|id| !packet.task.inputs.iter().any(|a| &a.logical_id == id))
        || packet
            .task
            .outputs
            .keys()
            .any(|n| !portable(n) || n.contains('/'))
    {
        return Err(error(
            ErrorKind::Invalid,
            "provider-explicit-inputs-outputs",
        ));
    }
    for (name, limit) in [
        (REQUEST, c.http.request_byte_limit),
        (RESPONSE, c.http.response_byte_limit),
        (CONFIG, reel_workflow::MAX_CONTROL_BYTES as u64),
    ] {
        if packet.task.outputs.get(name).is_none_or(|o| {
            o.media_type != "application/json" || !o.required || o.max_bytes < limit
        }) {
            return Err(error(
                ErrorKind::Invalid,
                "provider-declared-evidence-required",
            ));
        }
    }
    let creative = packet
        .task
        .outputs
        .iter()
        .filter(|(n, _)| ![REQUEST, RESPONSE, CONFIG].contains(&n.as_str()))
        .try_fold(0u64, |n, (_, o)| n.checked_add(o.max_bytes));
    let required = c
        .http
        .request_byte_limit
        .checked_mul(2)
        .and_then(|n| {
            c.http
                .response_byte_limit
                .checked_mul(2)
                .and_then(|r| n.checked_add(r))
        })
        .and_then(|n| creative.and_then(|r| n.checked_add(r)))
        .and_then(|n| n.checked_add(8 * reel_workflow::MAX_CONTROL_BYTES as u64));
    if required.is_none_or(|n| n > c.workspace.maximum_bytes) {
        return Err(error(ErrorKind::Budget, "provider-workspace-reservation"));
    }
    Ok(())
}
fn unique_outputs<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, Encoded>, D::Error> {
    struct Unique;
    impl<'de> serde::de::Visitor<'de> for Unique {
        type Value = BTreeMap<String, Encoded>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("bounded unique provider outputs")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, Encoded>()? {
                if values.len() >= 128 || values.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom(
                        "duplicate or excessive provider outputs",
                    ));
                }
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(Unique)
}
fn operation(ticket: &Ticket) -> Result<String> {
    reel_workflow::canonical_digest(
        &serde_json::json!({"schema":"reel.workflow-provider-operation.v1","admission":ticket.admission.operation_identity()?,"packet":ticket.packet,"config":ticket.config}),
    )
}
fn intent(
    packet: &Packet,
    receipt: &Receipt,
    config: &Config,
    batch: &graph::PinnedFile,
) -> Result<Plan> {
    let portable = serde_json::json!({"schema":config.schema,"provider":config.provider,"model":config.model,"send_inputs":config.send_inputs,"http":config.http});
    let operation = reel_workflow::canonical_digest(
        &serde_json::json!({"schema":"reel.workflow-provider-workspace.v1","admission":receipt.operation_identity()?,"packet":packet,"config":portable,"batch":batch}),
    )?;
    let raw = json(
        &serde_json::json!({"schema":"reel.workflow-provider-invocation.v1","packet":packet,"admission":receipt,"config":portable,"batch":batch}),
    )?;
    Ok(Plan {
        manifest: Manifest {
            schema: "reel.workflow-provider-intent.v1".into(),
            operation,
            files: BTreeMap::from([(
                "invocation.json".into(),
                Entry {
                    object: Object::from_bytes(&raw),
                    input: None,
                },
            )]),
        },
        controls: BTreeMap::from([("invocation.json".into(), raw)]),
    })
}
fn preserve(path: &Path, bytes: &[u8], maximum: u64) -> Result<()> {
    if bytes.len() as u64 > maximum {
        return Err(error(ErrorKind::Budget, "provider-file-bytes"));
    }
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file
            .write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| error(ErrorKind::NeedsRecovery, "provider-write-preserve"))?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(io_error(e)),
    }
    let (actual, _held) = read_held(path, maximum)?;
    if actual != bytes {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "provider-existing-bytes-preserve",
        ));
    }
    Ok(())
}
pub fn prepare_file(
    plan: &reel_workflow::Plan,
    bindings: &graph::Bindings,
    batch: &graph::PinnedFile,
    id: &str,
    config: &Config,
    path: &Path,
) -> Result<()> {
    let selected = graph::select(plan, bindings, batch, id)?;
    check(&selected.task.ticket.packet, config)?;
    let _runtime = handoff::runtime(&selected.profile.runtime)?;
    if selected.observed.state != State::Waiting {
        return Err(error(ErrorKind::Conflict, "provider-waiting-required"));
    }
    let parent = open_workspace_directory(
        path.parent()
            .ok_or_else(|| error(ErrorKind::Invalid, "provider-ticket-parent"))?,
    )
    .map_err(scoped_error)?;
    check_parent(
        parent.path(),
        &selected.profile.source,
        &selected.profile.admission,
    )?;
    let scratch = open_workspace_directory(&config.workspace.parent).map_err(scoped_error)?;
    check_parent(
        scratch.path(),
        &selected.profile.source,
        &selected.profile.admission,
    )?;
    for root in [
        &selected.task.journal,
        &selected.profile.acknowledgements,
        &selected.profile.source.snapshot.repository,
        &selected.task.workspace,
    ] {
        let root = root.canonicalize().map_err(io_error)?;
        if parent.path().starts_with(&root) || scratch.path().starts_with(&root) {
            return Err(error(ErrorKind::Denied, "provider-workspace-overlap"));
        }
    }
    if !path.is_absolute()
        || path
            .file_name()
            .and_then(|n| n.to_str())
            .is_none_or(|n| !portable(n))
        || parent.path().starts_with(scratch.path())
        || scratch.path().starts_with(parent.path())
    {
        return Err(error(ErrorKind::Denied, "provider-ticket-namespace"));
    }
    let _control = handoff::lock_waiting(
        &selected.task.journal,
        &selected.profile,
        &selected.task.ticket.packet,
    )?;
    let _claims = admission::Prepared::open(
        &selected.task.ticket.packet,
        &selected.profile.source,
        &selected.profile.admission,
        &selected.task.ticket.attempt_id,
    )?
    .reopen(&selected.profile.binding.admission)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error)?;
    let base = intent(
        &selected.task.ticket.packet,
        &selected.profile.binding.admission,
        config,
        batch,
    )?;
    let (workspace, reservation, owner) = prepare_owned(
        &config.workspace,
        &selected.profile.admission.scope.coordination_root,
        &base,
    )?;
    let mut config: Config = reel_workflow::decode(&json(config)?)?;
    config.workspace = workspace;
    let ticket = Ticket {
        schema: "reel.workflow-provider-ticket.v1".into(),
        config,
        reservation,
        owner,
        admission: selected.profile.binding.admission.clone(),
        batch: batch.clone(),
        packet: selected.task.ticket.packet.clone(),
    };
    file.write_all(&ticket.canonical_bytes()?)
        .and_then(|_| file.sync_all())
        .map_err(|_| error(ErrorKind::NeedsRecovery, "provider-ticket-preserve"))
}
pub struct Outcome {
    pub state: State,
    pub called: bool,
}
pub fn run_file(
    plan: &reel_workflow::Plan,
    bindings: &graph::Bindings,
    path: &Path,
    pin: &str,
) -> Result<Outcome> {
    let (raw, _ticket_guard) = read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if reel_workflow::digest(&raw) != pin {
        return Err(error(ErrorKind::Integrity, "provider-ticket-pin"));
    }
    let ticket: Ticket = reel_workflow::decode(&raw)?;
    if ticket.schema != "reel.workflow-provider-ticket.v1" {
        return Err(error(ErrorKind::Invalid, "provider-ticket-schema"));
    }
    check(&ticket.packet, &ticket.config)?;
    let selected = graph::select(plan, bindings, &ticket.batch, &ticket.packet.task.id)?;
    let _runtime = handoff::runtime(&selected.profile.runtime)?;
    if selected.task.ticket.packet != ticket.packet
        || selected.profile.binding.admission.operation_identity()?
            != ticket.admission.operation_identity()?
    {
        return Err(error(ErrorKind::Conflict, "provider-original-job"));
    }
    let base = intent(
        &ticket.packet,
        &ticket.admission,
        &ticket.config,
        &ticket.batch,
    )?;
    let view = open_owner(&ticket.config.workspace, &ticket.owner, &base)?;
    let config = configuration(
        &ticket.config.workspace,
        &selected.profile.admission.scope.coordination_root,
    )?;
    let _reservation = capacity::resume_preserved_on(
        &config,
        &ticket.config.workspace.parent,
        &ticket.reservation.id,
        &ticket.reservation.volume,
        ticket.config.workspace.maximum_bytes,
    )
    .map_err(scoped_error)?;
    let root = view.root.path();
    let op = operation(&ticket)?;
    preserve(
        &root.join("invocation.json"),
        &base.controls["invocation.json"],
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    let request_path = root.join("request.json");
    let response_path = root.join("response.bin");
    let complete_path = root.join("response-complete.json");
    let call_path = root.join("call-intent.json");
    let registry = selected
        .profile
        .admission
        .scope
        .coordination_root
        .join("workflow-provider-calls");
    match fs::create_dir(&registry) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(io_error(e)),
    };
    let registry_guard = open_workspace_directory(&registry).map_err(scoped_error)?;
    let registry_path = registry_guard
        .path()
        .join(format!("{}.json", ticket.admission.operation_identity()?));
    let owner_raw = json(
        &serde_json::json!({"schema":"reel.workflow-provider-call-owner.v1","admission":ticket.admission.operation_identity()?,"operation":op,"workspace_identity":view.root.identity(),"workspace":root}),
    )?;
    let called = !call_path.exists();
    if called && registry_path.exists() {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "provider-original-call-owner-exists",
        ));
    }
    if !called {
        let (original, _held) = read_held(&registry_path, reel_workflow::MAX_CONTROL_BYTES as u64)
            .map_err(|_| {
                error(
                    ErrorKind::NeedsRecovery,
                    "provider-original-call-owner-missing",
                )
            })?;
        if original != owner_raw {
            return Err(error(ErrorKind::Conflict, "provider-original-call-owner"));
        }
    }
    if called {
        if selected.observed.state != State::Waiting {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "provider-original-call-missing",
            ));
        }
        let control =
            handoff::lock_waiting(&selected.task.journal, &selected.profile, &ticket.packet)?;
        let claims = admission::Prepared::open(
            &ticket.packet,
            &selected.profile.source,
            &selected.profile.admission,
            &ticket.admission.attempt_id,
        )?
        .reopen(&ticket.admission)?;
        let mut native =
            NativeInputs::from_claims(&ticket.packet, &selected.profile.source, claims)?;
        let mut buffers = BTreeMap::new();
        let mut total = 0u64;
        for asset in &ticket.packet.task.inputs {
            let bytes = native.read(asset, asset.bytes)?;
            if ticket.config.send_inputs.contains(&asset.logical_id) {
                total = total
                    .checked_add(asset.bytes)
                    .ok_or_else(|| error(ErrorKind::Budget, "provider-input-bytes"))?;
                if total > ticket.config.http.request_byte_limit {
                    return Err(error(ErrorKind::Budget, "provider-input-bytes"));
                }
                buffers.insert(
                    asset.logical_id.clone(),
                    serde_json::json!({"artifact":asset,"base64":STANDARD.encode(bytes)}),
                );
            }
        }
        let inputs: Vec<_> = ticket
            .config
            .send_inputs
            .iter()
            .map(|id| &buffers[id])
            .collect();
        let outputs: BTreeMap<_, _> = ticket
            .packet
            .task
            .outputs
            .iter()
            .filter(|(n, _)| ![REQUEST, RESPONSE, CONFIG].contains(&n.as_str()))
            .collect();
        let body=serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-proxy-request.v1","operation_id":op,"provider":ticket.config.provider,"model":ticket.config.model,
            "task":{"id":ticket.packet.task.id,"task_key":ticket.packet.task_key,"attempt_id":ticket.admission.attempt_id,"operation":ticket.packet.task.operation,"language":ticket.packet.task.language,"prompt":ticket.packet.task.prompt,"outputs":outputs},"inputs":inputs})).map_err(|_|error(ErrorKind::Invalid,"provider-request-json"))?;
        let prepared = ticket
            .config
            .http
            .prepare(&op, &body, |name| std::env::var(name).ok())?;
        preserve(&request_path, &body, ticket.config.http.request_byte_limit)?;
        let mut response = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&response_path)
            .map_err(|_| {
                error(
                    ErrorKind::NeedsRecovery,
                    "provider-original-response-preserve",
                )
            })?;
        let reply = prepared.send_captured(
            |intent| {
                let mut global = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&registry_path)
                    .map_err(|_| {
                        error(
                            ErrorKind::NeedsRecovery,
                            "provider-original-call-owner-exists",
                        )
                    })?;
                global
                    .write_all(&owner_raw)
                    .and_then(|_| global.sync_all())
                    .map_err(|_| error(ErrorKind::NeedsRecovery, "provider-call-owner-preserve"))?;
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&call_path)
                    .map_err(|_| {
                        error(ErrorKind::NeedsRecovery, "provider-original-call-preserve")
                    })?;
                file.write_all(&json(intent)?)
                    .and_then(|_| file.sync_all())
                    .map_err(|_| error(ErrorKind::NeedsRecovery, "provider-call-intent-preserve"))
            },
            |bytes| {
                response
                    .write_all(bytes)
                    .and_then(|_| response.sync_all())
                    .map_err(|_| {
                        error(
                            ErrorKind::NeedsRecovery,
                            "provider-response-prefix-preserve",
                        )
                    })
            },
        )?;
        preserve(
            &complete_path,
            &json(&HttpComplete {
                schema: "reel.workflow-provider-http-complete.v1".into(),
                operation_id: op.clone(),
                status: reply.status,
                bytes: reply.body.len() as u64,
                sha256: reply.body_sha256,
            })?,
            reel_workflow::MAX_CONTROL_BYTES as u64,
        )?;
        drop(native);
        drop(control);
    }
    let (call, _call_guard) = read_held(&call_path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    let call: serde_json::Value = reel_workflow::decode(&call)?;
    let (request, _request_guard) =
        read_held(&request_path, ticket.config.http.request_byte_limit)?;
    if call["operation_id"] != op
        || call["request_sha256"] != reel_workflow::digest(&request)
        || call["config_sha256"] != reel_workflow::canonical_digest(&ticket.config.http)?
    {
        return Err(error(ErrorKind::Conflict, "provider-original-call-binding"));
    }
    let (complete, _complete_guard) =
        read_held(&complete_path, reel_workflow::MAX_CONTROL_BYTES as u64)
            .map_err(|_| error(ErrorKind::NeedsRecovery, "provider-original-call-uncertain"))?;
    let complete: HttpComplete = reel_workflow::decode(&complete)?;
    let (response, _response_guard) =
        read_held(&response_path, ticket.config.http.response_byte_limit)?;
    if complete.schema != "reel.workflow-provider-http-complete.v1"
        || complete.operation_id != op
        || complete.bytes != response.len() as u64
        || complete.sha256 != reel_workflow::digest(&response)
    {
        return Err(error(ErrorKind::Integrity, "provider-response-binding"));
    }
    if !(200..300).contains(&complete.status) {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "provider-http-status-reconcile",
        ));
    }
    let reply: Reply = serde_json::from_slice(&response)
        .map_err(|_| error(ErrorKind::Integrity, "provider-response-json"))?;
    if reply.schema != "reel.workflow-proxy-response.v1"
        || reply.operation_id != op
        || reply.provider != ticket.config.provider
        || reply.model != ticket.config.model
    {
        return Err(error(ErrorKind::Conflict, "provider-response-context"));
    }
    let mut files = BTreeMap::from([
        (REQUEST.to_string(), request),
        (RESPONSE.to_string(), response),
        (CONFIG.to_string(), json(&ticket.config)?),
    ]);
    let mut total = 0u64;
    for (name, output) in reply.outputs {
        let contract = ticket
            .packet
            .task
            .outputs
            .get(&name)
            .ok_or_else(|| error(ErrorKind::Invalid, "provider-undeclared-output"))?;
        if [REQUEST, RESPONSE, CONFIG].contains(&name.as_str())
            || output.media_type != contract.media_type
            || output.base64.len() as u64
                > contract
                    .max_bytes
                    .saturating_mul(4)
                    .div_ceil(3)
                    .saturating_add(8)
        {
            return Err(error(ErrorKind::Invalid, "provider-output-contract"));
        }
        let bytes = STANDARD
            .decode(output.base64)
            .map_err(|_| error(ErrorKind::Integrity, "provider-output-base64"))?;
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| error(ErrorKind::Budget, "provider-output-bytes"))?;
        if bytes.len() as u64 > contract.max_bytes || total > 128 * 1024 * 1024 {
            return Err(error(ErrorKind::Budget, "provider-output-bytes"));
        }
        files.insert(name, bytes);
    }
    let mut artifacts = BTreeMap::new();
    let packet = root.join("packet");
    match fs::create_dir(&packet) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(io_error(e)),
    };
    let _packet_guard = open_workspace_directory(&packet).map_err(scoped_error)?;
    for (name, bytes) in &files {
        if !portable(name) || name.contains('/') {
            return Err(error(ErrorKind::Invalid, "provider-flat-output-name"));
        }
        let contract = ticket
            .packet
            .task
            .outputs
            .get(name)
            .ok_or_else(|| error(ErrorKind::Invalid, "provider-output-contract"))?;
        let artifact = Artifact {
            logical_id: format!(
                "workflow/provider/{}/{}/{}",
                ticket.packet.task_key, ticket.admission.attempt_id, name
            ),
            sha256: reel_workflow::digest(bytes),
            bytes: bytes.len() as u64,
            media_type: contract.media_type.clone(),
        };
        preserve(&packet.join(name), bytes, contract.max_bytes)?;
        artifacts.insert(name.clone(), artifact);
    }
    let result = ReturnedResult {
        schema: reel_workflow::RESULT_SCHEMA.into(),
        task_id: ticket.packet.task.id.clone(),
        task_key: ticket.packet.task_key.clone(),
        attempt_id: ticket.admission.attempt_id.clone(),
        artifacts,
        provenance: Provenance {
            mode: Mode::Direct,
            provider: Some(ticket.config.provider.clone()),
            model: Some(ticket.config.model.clone()),
            executable: None,
        },
        worker_receipt: None,
    };
    reel_workflow::check_result(&ticket.packet, &result)?;
    let returns = crate::response::Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: packet,
        files: result
            .artifacts
            .keys()
            .map(|n| (n.clone(), n.clone()))
            .collect(),
    };
    preserve(
        &root.join("result.json"),
        &json(&result)?,
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    preserve(
        &root.join("returns.json"),
        &json(&returns)?,
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    let state = graph::import_ready(
        plan,
        bindings,
        &ticket.batch,
        &ticket.packet.task.id,
        result,
        &returns,
    )?;
    Ok(Outcome { state, called })
}
