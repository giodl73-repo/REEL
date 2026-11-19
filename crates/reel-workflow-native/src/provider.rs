//! Protected HTTP proxy calls. Original call uncertainty never launches another.
use super::*;
use crate::{graph, handoff};
use base64::{Engine, engine::general_purpose::STANDARD};
use reel_workflow::{Mode, Operation, Provenance, ReturnedResult, handoff::State};

const REQUEST: &str = "provider-request.json";
const RESPONSE: &str = "provider-response.json";
const CONFIG: &str = "provider-config.json";
fn proxy_request(
    op: &str,
    provider: &str,
    model: &str,
    task: serde_json::Value,
    inputs: serde_json::Value,
    brief: Option<serde_json::Value>,
) -> Result<Vec<u8>> {
    let mut body = serde_json::json!({"schema":"reel.workflow-proxy-request.v1","operation_id":op,"provider":provider,"model":model,"task":task,"inputs":inputs});
    if let Some(brief) = brief {
        body["schema"] = serde_json::json!("reel.workflow-proxy-request.v2");
        body["review_brief"] = brief;
    }
    serde_json::to_vec(&body).map_err(|_| error(ErrorKind::Invalid, "provider-request-json"))
}
#[cfg(test)]
mod review_wire_tests {
    use super::*;
    #[test]
    fn legacy_proxy_bytes_stay_v1_and_review_extension_uses_v2() {
        let task = serde_json::json!({"id":"synthetic","operation":"text-proposal"});
        let inputs = serde_json::json!([]);
        let legacy=serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-proxy-request.v1","operation_id":"original","provider":"proxy","model":"declared","task":task,"inputs":inputs})).unwrap();
        assert_eq!(
            proxy_request(
                "original",
                "proxy",
                "declared",
                task.clone(),
                inputs.clone(),
                None
            )
            .unwrap(),
            legacy
        );
        let brief = serde_json::json!({"schema":"reel.workflow-review-brief.v1","return_schema":{"type":"object"}});
        let body: serde_json::Value = serde_json::from_slice(
            &proxy_request(
                "original",
                "proxy",
                "declared",
                task,
                inputs,
                Some(brief.clone()),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(body["schema"], "reel.workflow-proxy-request.v2");
        assert_eq!(body["review_brief"], brief);
        assert_eq!(body["operation_id"], "original");
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub provider: String,
    pub model: String,
    pub send_inputs: Vec<String>,
    pub http: reel_workflow_http::Config,
    pub workspace: Profile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<Service>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Service {
    ElevenlabsTimed {
        spec: reel_workflow_http::elevenlabs::Spec,
    },
    OpenaiImage {
        spec: reel_workflow_http::openai_image::Spec,
    },
    OpenaiText {
        spec: reel_workflow_http::openai_text::Spec,
    },
}
#[cfg(test)]
mod image_context_tests {
    use super::*;
    #[test]
    fn image_api_refuses_ungranted_inputs_other_operations_and_unknown_outputs() {
        let reference = reel_workflow::Artifact {
            logical_id: "reference".into(),
            sha256: reel_workflow::digest(b"metadata-only"),
            bytes: 13,
            media_type: "image/png".into(),
        };
        let task = reel_workflow::Task {
            id: "image".into(),
            native_inputs: None,
            upstream_inputs: None,
            operation: Operation::ImageCandidate,
            tool: None,
            language: None,
            prompt: "Synthetic prompt".into(),
            inputs: vec![reference],
            depends_on: vec![],
            outputs: [
                ("candidate.png", "image/png"),
                ("image.json", "application/json"),
                ("http.json", "application/json"),
                (REQUEST, "application/json"),
                (RESPONSE, "application/json"),
                (CONFIG, "application/json"),
            ]
            .into_iter()
            .map(|(n, m)| {
                (
                    n.into(),
                    reel_workflow::OutputContract {
                        media_type: m.into(),
                        max_bytes: 1024 * 1024,
                        required: true,
                    },
                )
            })
            .collect(),
        };
        let mut packet = reel_workflow::make_packet(
            &reel_workflow::Plan {
                schema: reel_workflow::PLAN_SCHEMA.into(),
                task_snapshots: Default::default(),
                project: "synthetic".into(),
                source_commit: "a".repeat(40),
                tasks: vec![task],
            },
            "image",
            BTreeMap::new(),
        )
        .unwrap();
        let mut config=Config {schema:"reel.workflow-provider-proxy.v1".into(),provider:"openai".into(),model:"gpt-image-1.5".into(),send_inputs:vec!["reference".into()],http:reel_workflow_http::Config {schema:"reel.workflow-http.v1".into(),endpoint:"http://127.0.0.1:9/v1/images/edits".into(),timeout_seconds:1,request_byte_limit:1024*1024,response_byte_limit:1024*1024,secret_headers:BTreeMap::new(),allow_loopback_http:true},workspace:Profile {schema:"reel.workflow-staging.v1".into(),parent:"not-opened".into(),maximum_bytes:16*1024*1024,capacity_policy:Default::default()},service:Some(Service::OpenaiImage {spec:serde_json::from_value(serde_json::json!({"schema":"reel.openai-image.v1","references":["reference"],"mask":null,"size":[1024,1024],"quality":"high","background":"opaque"})).unwrap()})};
        check(&packet, &config).unwrap();
        config.send_inputs.clear();
        assert_eq!(check(&packet, &config).unwrap_err().kind, ErrorKind::Denied);
        config.send_inputs = vec!["reference".into()];
        let mut private = packet.task.inputs[0].clone();
        private.logical_id = "private-unused".into();
        packet.task.inputs.push(private);
        config.send_inputs.push("private-unused".into());
        assert_eq!(
            check(&packet, &config).unwrap_err().code,
            "openai-image-native-context"
        );
        config.send_inputs.pop();
        packet.task.operation = Operation::ReviewReport;
        assert_eq!(check(&packet, &config).unwrap_err().kind, ErrorKind::Denied);
        packet.task.operation = Operation::ImageCandidate;
        packet.task.outputs.insert(
            "undeclared-by-service".into(),
            packet.task.outputs["image.json"].clone(),
        );
        assert_eq!(
            check(&packet, &config).unwrap_err().code,
            "openai-image-output-contract"
        );
        config.service = Some(Service::OpenaiText { spec: serde_json::from_value(serde_json::json!({"schema":"reel.openai-text.v1","format":"text","output":"candidate.txt","max_output_tokens":1024})).unwrap() });
        config.model = "declared-text-model".into();
        config.http.endpoint = "http://127.0.0.1:9/v1/responses".into();
        packet.task.operation = Operation::TextProposal;
        packet.task.outputs = [
            ("candidate.txt", "text/plain"),
            ("text.json", "application/json"),
            ("http.json", "application/json"),
            (REQUEST, "application/json"),
            (RESPONSE, "application/json"),
            (CONFIG, "application/json"),
        ]
        .into_iter()
        .map(|(n, m)| {
            (
                n.into(),
                reel_workflow::OutputContract {
                    media_type: m.into(),
                    max_bytes: 1024 * 1024,
                    required: true,
                },
            )
        })
        .collect();
        check(&packet, &config).unwrap();
        packet.task.inputs[0].media_type = "video/mp4".into();
        assert_eq!(
            check(&packet, &config).unwrap_err().code,
            "openai-text-native-context"
        );
        config.send_inputs.clear();
        check(&packet, &config).unwrap();
        packet.task.operation = Operation::ImageCandidate;
        assert_eq!(check(&packet, &config).unwrap_err().kind, ErrorKind::Denied);
        packet.task.operation = Operation::TextProposal;
        packet
            .task
            .outputs
            .get_mut("candidate.txt")
            .unwrap()
            .media_type = "application/json".into();
        assert_eq!(
            check(&packet, &config).unwrap_err().code,
            "openai-text-output-contract"
        );
    }
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
    if let Some(Service::ElevenlabsTimed { spec }) = &c.service {
        spec.check(&c.model)?;
        spec.check_endpoint(&c.http)?;
        if packet.task.operation != Operation::SpeechCandidate
            || c.provider != "elevenlabs"
            || packet.task.language.as_deref() != Some(spec.language.as_str())
            || c.send_inputs != [spec.text_input.clone()]
            || [
                &spec.text_input,
                &spec.models_input,
                &spec.admission_input,
                &spec.decision_input,
            ]
            .iter()
            .any(|id| !packet.task.inputs.iter().any(|a| &a.logical_id == *id))
        {
            return Err(error(ErrorKind::Invalid, "elevenlabs-native-context"));
        }
        for (name, media) in reel_workflow_http::elevenlabs::OUTPUTS
            .into_iter()
            .chain([("http.json", "application/json")])
        {
            if packet
                .task
                .outputs
                .get(name)
                .is_none_or(|o| !o.required || o.media_type != media)
            {
                return Err(error(ErrorKind::Invalid, "elevenlabs-declared-outputs"));
            }
        }
    }
    if let Some(Service::OpenaiImage { spec }) = &c.service {
        spec.check(&c.model)?;
        spec.check_endpoint(&c.http)?;
        if packet.task.operation != Operation::ImageCandidate
            || c.provider != "openai"
            || c.send_inputs.iter().cloned().collect::<BTreeSet<_>>() != spec.egress_inputs()
            || spec.egress_inputs().iter().any(|id| {
                !packet
                    .task
                    .inputs
                    .iter()
                    .any(|a| a.logical_id == *id && a.media_type == "image/png")
            })
        {
            return Err(error(ErrorKind::Denied, "openai-image-native-context"));
        }
        let outputs = BTreeSet::from([
            "candidate.png",
            "image.json",
            "http.json",
            REQUEST,
            RESPONSE,
            CONFIG,
        ]);
        if packet
            .task
            .outputs
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != outputs
            || packet.task.outputs.iter().any(|(name, c)| {
                !c.required
                    || c.media_type
                        != if name == "candidate.png" {
                            "image/png"
                        } else {
                            "application/json"
                        }
            })
            || packet.task.outputs["candidate.png"].max_bytes > 64 * 1024 * 1024
            || ["image.json", "http.json"].iter().any(|n| {
                packet.task.outputs[*n].max_bytes > reel_workflow::MAX_CONTROL_BYTES as u64
            })
        {
            return Err(error(ErrorKind::Invalid, "openai-image-output-contract"));
        }
    }
    if let Some(Service::OpenaiText { spec }) = &c.service {
        spec.check(&c.model)?;
        spec.check_endpoint(&c.http)?;
        let operation = match spec.format {
            reel_workflow_http::openai_text::Format::Text => Operation::TextProposal,
            reel_workflow_http::openai_text::Format::ReviewReport => Operation::ReviewReport,
        };
        if c.provider != "openai"
            || packet.task.operation != operation
            || c.send_inputs.iter().any(|id| {
                packet
                    .task
                    .inputs
                    .iter()
                    .find(|a| &a.logical_id == id)
                    .is_none_or(|a| {
                        !["text/plain", "application/json", "image/png"]
                            .contains(&a.media_type.as_str())
                    })
            })
        {
            return Err(error(ErrorKind::Denied, "openai-text-native-context"));
        }
        let names = BTreeSet::from([
            spec.output.as_str(),
            "text.json",
            "http.json",
            REQUEST,
            RESPONSE,
            CONFIG,
        ]);
        if packet
            .task
            .outputs
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != names
            || packet.task.outputs.iter().any(|(n, o)| {
                !o.required
                    || o.media_type
                        != if n == &spec.output {
                            spec.media_type()
                        } else {
                            "application/json"
                        }
            })
            || [spec.output.as_str(), "text.json", "http.json"]
                .iter()
                .any(|n| {
                    packet.task.outputs[*n].max_bytes > reel_workflow::MAX_CONTROL_BYTES as u64
                })
        {
            return Err(error(ErrorKind::Invalid, "openai-text-output-contract"));
        }
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
    let mut portable = serde_json::json!({"schema":config.schema,"provider":config.provider,"model":config.model,"send_inputs":config.send_inputs,"http":config.http});
    if let Some(service) = &config.service {
        portable["service"] = serde_json::to_value(service)
            .map_err(|_| error(ErrorKind::Invalid, "provider-service-json"))?;
    }
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
        let review_brief = if ticket.packet.task.operation == Operation::ReviewReport {
            let bound = reel_workflow::review::bind(&ticket.packet, &mut native)?;
            Some(bound.brief(
                &ticket.admission.attempt_id,
                &ticket.config.send_inputs.iter().cloned().collect(),
            )?)
        } else {
            None
        };
        let mut buffers = BTreeMap::new();
        let mut raw_inputs = BTreeMap::new();
        let mut total = 0u64;
        for asset in &ticket.packet.task.inputs {
            let bytes = native.read(asset, asset.bytes)?;
            if ticket.config.service.is_some() {
                total = total
                    .checked_add(asset.bytes)
                    .ok_or_else(|| error(ErrorKind::Budget, "elevenlabs-input-bytes"))?;
                if total > 128 * 1024 * 1024 {
                    return Err(error(ErrorKind::Budget, "elevenlabs-input-bytes"));
                }
                raw_inputs.insert(asset.logical_id.clone(), bytes);
                continue;
            }
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
        let body = if let Some(Service::ElevenlabsTimed { spec }) = &ticket.config.service {
            let inputs = ticket
                .packet
                .task
                .inputs
                .iter()
                .map(|a| {
                    (
                        a.logical_id.clone(),
                        reel_workflow_http::elevenlabs::Input {
                            artifact: a,
                            bytes: &raw_inputs[&a.logical_id][..],
                        },
                    )
                })
                .collect();
            spec.request(&ticket.config.model, &inputs)?
        } else if let Some(Service::OpenaiImage { spec }) = &ticket.config.service {
            let inputs = ticket
                .packet
                .task
                .inputs
                .iter()
                .map(|a| {
                    (
                        a.logical_id.clone(),
                        reel_workflow_http::openai_image::Input {
                            artifact: a,
                            bytes: &raw_inputs[&a.logical_id],
                        },
                    )
                })
                .collect();
            spec.request(
                &ticket.config.model,
                &ticket.packet.task.prompt,
                &inputs,
                ticket.config.http.request_byte_limit,
            )?
        } else if let Some(Service::OpenaiText { spec }) = &ticket.config.service {
            let inputs = ticket
                .packet
                .task
                .inputs
                .iter()
                .filter(|a| ticket.config.send_inputs.contains(&a.logical_id))
                .map(|a| {
                    (
                        a.logical_id.clone(),
                        reel_workflow_http::openai_text::Input {
                            artifact: a,
                            bytes: &raw_inputs[&a.logical_id],
                        },
                    )
                })
                .collect();
            spec.request(
                &ticket.config.model,
                &ticket.packet.task.prompt,
                &inputs,
                review_brief.as_ref(),
                ticket.config.http.request_byte_limit,
            )?
        } else {
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
            proxy_request(
                &op,
                &ticket.config.provider,
                &ticket.config.model,
                serde_json::json!({"id":ticket.packet.task.id,"task_key":ticket.packet.task_key,"attempt_id":ticket.admission.attempt_id,"operation":ticket.packet.task.operation,"language":ticket.packet.task.language,"prompt":ticket.packet.task.prompt,"outputs":outputs}),
                serde_json::json!(inputs),
                review_brief
                    .as_ref()
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|_| error(ErrorKind::Invalid, "provider-review-brief-json"))?,
            )?
        };
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
        let reply = prepared.send_observed(
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
            |head| {
                preserve(
                    &root.join("http-head.json"),
                    &json(head)?,
                    reel_workflow::MAX_CONTROL_BYTES as u64,
                )
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
    let mut creative = if let Some(Service::ElevenlabsTimed { spec }) = &ticket.config.service {
        let request_value: serde_json::Value = serde_json::from_slice(&request)
            .map_err(|_| error(ErrorKind::Integrity, "elevenlabs-original-request"))?;
        let text = request_value["text"]
            .as_str()
            .ok_or_else(|| error(ErrorKind::Integrity, "elevenlabs-original-text"))?;
        let mut outputs = spec.normalize(
            &ticket.config.model,
            text.as_bytes(),
            &response,
            ticket.packet.task.outputs["speech.wav"].max_bytes,
        )?;
        let (head, _guard) = read_held(
            &root.join("http-head.json"),
            reel_workflow::MAX_CONTROL_BYTES as u64,
        )?;
        let observed: reel_workflow_http::ResponseHead = reel_workflow::decode(&head)?;
        if observed.schema != "reel.workflow-http-head.v1" || observed.status != complete.status {
            return Err(error(ErrorKind::Integrity, "elevenlabs-http-head"));
        }
        outputs.insert("http.json".into(), head);
        outputs
    } else if let Some(Service::OpenaiImage { spec }) = &ticket.config.service {
        let mut outputs = spec.normalize(
            &ticket.config.model,
            &ticket.packet.task.prompt,
            &response,
            ticket.packet.task.outputs["candidate.png"].max_bytes,
        )?;
        let (head, _guard) = read_held(
            &root.join("http-head.json"),
            reel_workflow::MAX_CONTROL_BYTES as u64,
        )?;
        let observed: reel_workflow_http::ResponseHead = reel_workflow::decode(&head)?;
        if observed.schema != "reel.workflow-http-head.v1" || observed.status != complete.status {
            return Err(error(ErrorKind::Integrity, "openai-image-http-head"));
        }
        outputs.insert("http.json".into(), head);
        outputs
    } else if let Some(Service::OpenaiText { spec }) = &ticket.config.service {
        let mut outputs = spec.normalize(
            &ticket.config.model,
            &request,
            &response,
            ticket.packet.task.outputs[&spec.output].max_bytes,
        )?;
        let (head, _guard) = read_held(
            &root.join("http-head.json"),
            reel_workflow::MAX_CONTROL_BYTES as u64,
        )?;
        let observed: reel_workflow_http::ResponseHead = reel_workflow::decode(&head)?;
        if observed.schema != "reel.workflow-http-head.v1" || observed.status != complete.status {
            return Err(error(ErrorKind::Integrity, "openai-text-http-head"));
        }
        outputs.insert("http.json".into(), head);
        outputs
    } else {
        let reply: Reply = serde_json::from_slice(&response)
            .map_err(|_| error(ErrorKind::Integrity, "provider-response-json"))?;
        if reply.schema != "reel.workflow-proxy-response.v1"
            || reply.operation_id != op
            || reply.provider != ticket.config.provider
            || reply.model != ticket.config.model
        {
            return Err(error(ErrorKind::Conflict, "provider-response-context"));
        }
        let mut files = BTreeMap::new();
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
        files
    };
    let mut files = BTreeMap::from([
        (REQUEST.to_string(), request),
        (RESPONSE.to_string(), response),
        (CONFIG.to_string(), json(&ticket.config)?),
    ]);
    files.append(&mut creative);
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
        if bytes.len() as u64 > contract.max_bytes {
            return Err(error(ErrorKind::Budget, "provider-output-bytes"));
        }
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
