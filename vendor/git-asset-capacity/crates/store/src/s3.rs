//! Explicit source-preserving S3 conditional PUT/full-readback composition.
//! Local intent/completion reuse accounted publication. No retained custody.
use crate::{
    AckLevel, Key, Request,
    ledger::{CapacityLedger, JournalRef},
    local::{LocalStore, MAX_OBJECT_BYTES},
    orchestration::Place,
    upload_witness,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use git_asset_core::{Digest, Object, identity};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    io::{self, Read},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidPlan,
    InvalidAccess,
    Budget,
    Cancelled,
    Denied,
    Unavailable,
    Conflict,
    Integrity,
    MissingAcknowledgedRemote,
    NeedsRecovery,
    Busy,
    Unsupported,
}
impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidPlan => "invalid_plan",
            Self::InvalidAccess => "invalid_access",
            Self::Budget => "budget",
            Self::Cancelled => "cancelled",
            Self::Denied => "access_denied",
            Self::Unavailable => "remote_unavailable",
            Self::Conflict => "operation_conflict",
            Self::Integrity => "integrity",
            Self::MissingAcknowledgedRemote => "missing_acknowledged_remote",
            Self::NeedsRecovery => "needs_recovery",
            Self::Busy => "busy",
            Self::Unsupported => "unsupported",
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn store_error(e: crate::Error) -> Error {
    match e {
        crate::Error::Busy => Error::Busy,
        crate::Error::OperationConflict => Error::Conflict,
        crate::Error::Budget => Error::Budget,
        _ => Error::NeedsRecovery,
    }
}
fn witness_error(e: upload_witness::Error) -> Error {
    match e {
        upload_witness::Error::InvalidPlan => Error::InvalidPlan,
        upload_witness::Error::Budget => Error::Budget,
        upload_witness::Error::Conflict => Error::Conflict,
        upload_witness::Error::Busy => Error::Busy,
        upload_witness::Error::NeedsRecovery => Error::NeedsRecovery,
    }
}
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    let b = identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::InvalidPlan)?)
        .map_err(|_| Error::InvalidPlan)?;
    if b.len() > 16384 {
        return Err(Error::Budget);
    }
    Ok(b)
}
fn digest(v: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(v)?))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub instance: Digest,
    pub generation: u64,
    pub location: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub max_object_bytes: u64,
    pub transfer_bytes: u64,
    pub requests: u32,
    pub millis: u64,
}
impl Budget {
    pub fn validate(&self) -> Result<()> {
        if self.max_object_bytes == 0
            || self.max_object_bytes > MAX_OBJECT_BYTES
            || self.transfer_bytes == 0
            || self.transfer_bytes > 64 * 1024 * 1024
            || self.requests == 0
            || self.requests > 8
            || self.millis == 0
            || self.millis > 600000
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub operation: Key,
    pub owner: Key,
    pub source: Place,
    pub publication: Request,
    pub witness: Place,
    pub capacity: Digest,
    pub target: Target,
    pub context: Digest,
    pub budget: Budget,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    pub operation: Key,
    pub owner: Key,
    pub source: Place,
    pub publication: Request,
    pub witness: Place,
    pub capacity: Digest,
    pub target: Target,
    pub context: Digest,
    pub budget: Budget,
}
impl Plan {
    pub fn new(request: PlanRequest) -> Result<Self> {
        let PlanRequest {
            operation,
            owner,
            source,
            publication,
            witness,
            capacity,
            target,
            context,
            budget,
        } = request;
        let p = Self {
            schema: "tig.s3-publication-plan.design-1".into(),
            operation,
            owner,
            source,
            publication,
            witness,
            capacity,
            target,
            context,
            budget,
        };
        p.validate()?;
        Ok(p)
    }
    fn validate(&self) -> Result<()> {
        self.budget.validate()?;
        if self.schema != "tig.s3-publication-plan.design-1"
            || self.source.generation == 0
            || self.witness.generation == 0
            || self.target.generation == 0
            || self.source.instance == self.witness.instance
            || self.publication.object.bytes > self.budget.max_object_bytes
            || self.publication.required > AckLevel::ProcessRestart
        {
            return Err(Error::InvalidPlan);
        }
        Ok(())
    }
    pub fn id(&self) -> Result<Digest> {
        self.validate()?;
        digest(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        encode(self)
    }
    pub fn from_json(b: &[u8]) -> Result<Self> {
        if b.len() > 16384 {
            return Err(Error::Budget);
        }
        let p: Self =
            serde_json::from_value(identity::parse_json(b).map_err(|_| Error::InvalidPlan)?)
                .map_err(|_| Error::InvalidPlan)?;
        if p.to_json()? != b {
            return Err(Error::InvalidPlan);
        }
        Ok(p)
    }
    pub fn required_peak(&self, witness: &LocalStore) -> Result<u64> {
        let docs = Documents::new(self, witness)?;
        docs.packet.required_peak().map_err(witness_error)
    }
}

/// Sensitive access is deliberately neither serializable nor Debug-printable.
/// No ambient credentials/proxy/redirect or trace logger is installed.
pub struct Client {
    target: Target,
    object: Object,
    put: String,
    get: String,
    agent: ureq::Agent,
}
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Client")
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}
fn uri(value: &str, fixture: bool) -> Result<ureq::http::Uri> {
    if value.len() > 8192 || value.contains('#') {
        return Err(Error::InvalidAccess);
    }
    let uri: ureq::http::Uri = value.parse().map_err(|_| Error::InvalidAccess)?;
    let authority = uri.authority().ok_or(Error::InvalidAccess)?;
    if authority.as_str().contains('@')
        || uri.path().contains('%')
        || uri.path().split('/').any(|p| p == "." || p == "..")
    {
        return Err(Error::InvalidAccess);
    }
    let host = uri.host().ok_or(Error::InvalidAccess)?;
    if uri.scheme_str() != Some("https")
        && !(fixture
            && uri.scheme_str() == Some("http")
            && matches!(host, "127.0.0.1" | "[::1]" | "::1"))
    {
        return Err(Error::InvalidAccess);
    }
    // Only expiring authentication/operation annotations may be omitted from
    // location identity. In particular versionId/uploadId/partNumber would
    // change which representation or operation the URL addresses.
    let mut keys = std::collections::BTreeSet::new();
    for part in uri
        .query()
        .unwrap_or("")
        .split('&')
        .filter(|s| !s.is_empty())
    {
        let name = part
            .split_once('=')
            .ok_or(Error::InvalidAccess)?
            .0
            .to_ascii_lowercase();
        let known = matches!(
            name.as_str(),
            "x-amz-algorithm"
                | "x-amz-credential"
                | "x-amz-date"
                | "x-amz-expires"
                | "x-amz-signedheaders"
                | "x-amz-signature"
                | "x-amz-security-token"
                | "x-amz-content-sha256"
                | "x-amz-checksum-sha256"
                | "x-amz-sdk-checksum-algorithm"
                | "x-id"
        );
        if (!known && !(fixture && uri.scheme_str() == Some("http") && name == "fixture-access"))
            || !keys.insert(name)
        {
            return Err(Error::InvalidAccess);
        }
    }
    Ok(uri)
}
impl Client {
    pub fn presigned(
        instance: Digest,
        generation: u64,
        object: Object,
        put: &str,
        get: &str,
        loopback_fixture: bool,
    ) -> Result<Self> {
        if generation == 0 || object.bytes > MAX_OBJECT_BYTES {
            return Err(Error::InvalidAccess);
        }
        let p = uri(put, loopback_fixture)?;
        let g = uri(get, loopback_fixture)?;
        if p.scheme() != g.scheme()
            || p.authority() != g.authority()
            || p.path() != g.path()
            || !p.path().ends_with(&format!("/{}", object.sha256.as_str()))
        {
            return Err(Error::InvalidAccess);
        }
        let target = Target {
            instance,
            generation,
            location: digest(&(p.scheme_str(), p.authority().map(|a| a.as_str()), p.path()))?,
        };
        let agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .max_idle_connections(0)
            .max_response_header_size(16384)
            .input_buffer_size(65536)
            .output_buffer_size(65536)
            .build()
            .into();
        Ok(Self {
            target,
            object,
            put: put.into(),
            get: get.into(),
            agent,
        })
    }
    pub fn target(&self) -> &Target {
        &self.target
    }
    fn check(&self, p: &Plan) -> Result<()> {
        if self.target != p.target || self.object != p.publication.object {
            return Err(Error::InvalidAccess);
        }
        Ok(())
    }
    fn observe(&self, work: &mut Work<'_>) -> Result<Observation> {
        let time = work.request()?;
        let response = self
            .agent
            .get(&self.get)
            .header("accept-encoding", "identity")
            .config()
            .timeout_global(Some(time))
            .build()
            .call();
        work.check()?;
        let mut response = response.map_err(|_| Error::Unavailable)?;
        match response.status().as_u16() {
            404 => return Ok(Observation::Absent),
            401 | 403 => return Err(Error::Denied),
            200 => {}
            _ => return Err(Error::Unavailable),
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|h| h.as_bytes() != b"identity")
        {
            return Err(Error::Integrity);
        }
        if self.object.bytes > work.remaining {
            return Err(Error::Budget);
        }
        if let Some(length) = response.headers().get("content-length")
            && length.to_str().ok().and_then(|s| s.parse::<u64>().ok()) != Some(self.object.bytes)
        {
            return Err(Error::Integrity);
        }
        let mut reader = response.body_mut().as_reader();
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0; 65536];
        loop {
            work.check()?;
            let left = self
                .object
                .bytes
                .checked_add(1)
                .and_then(|n| n.checked_sub(total))
                .ok_or(Error::Integrity)?;
            let count = reader
                .read(&mut buffer[..left.min(65536) as usize])
                .map_err(|_| Error::Unavailable)?;
            if count == 0 {
                break;
            }
            work.consume(count as u64)?;
            total += count as u64;
            hasher.update(&buffer[..count]);
            if total > self.object.bytes {
                return Err(Error::Integrity);
            }
        }
        if total != self.object.bytes
            || hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                != self.object.sha256.as_str()
        {
            return Err(Error::Integrity);
        }
        work.check()?;
        Ok(Observation::Verified)
    }
    fn upload(&self, work: &mut Work<'_>, source: &mut dyn Read) -> Result<()> {
        let time = work.request()?;
        if self.object.bytes > work.remaining {
            return Err(Error::Budget);
        }
        let mut raw = [0u8; 32];
        for (i, byte) in raw.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&self.object.sha256.as_str()[i * 2..i * 2 + 2], 16)
                .map_err(|_| Error::InvalidPlan)?;
        }
        let mut reader = Counted {
            source: source.take(self.object.bytes),
            work,
        };
        let response = self
            .agent
            .put(&self.put)
            .header("if-none-match", "*")
            .header("x-amz-checksum-sha256", STANDARD.encode(raw))
            .header("content-length", self.object.bytes.to_string())
            .header("content-type", "application/octet-stream")
            .config()
            .timeout_global(Some(time))
            .build()
            .send(ureq::SendBody::from_reader(&mut reader));
        reader.work.check()?;
        let response = response.map_err(|_| Error::Unavailable)?;
        match response.status().as_u16() {
            200 | 412 => Ok(()),
            401 | 403 => Err(Error::Denied),
            409 => Err(Error::Conflict),
            _ => Err(Error::Unavailable),
        }
    }
}
struct Work<'a> {
    start: Instant,
    millis: u64,
    remaining: u64,
    requests: u32,
    cancel: &'a mut dyn FnMut() -> bool,
}
impl<'a> Work<'a> {
    fn new(b: &Budget, cancel: &'a mut dyn FnMut() -> bool) -> Self {
        Self {
            start: Instant::now(),
            millis: b.millis,
            remaining: b.transfer_bytes,
            requests: b.requests,
            cancel,
        }
    }
    fn check(&mut self) -> Result<()> {
        if (self.cancel)() {
            return Err(Error::Cancelled);
        }
        if self.start.elapsed() >= Duration::from_millis(self.millis) {
            return Err(Error::Budget);
        }
        Ok(())
    }
    fn request(&mut self) -> Result<Duration> {
        self.check()?;
        self.requests = self.requests.checked_sub(1).ok_or(Error::Budget)?;
        Duration::from_millis(self.millis)
            .checked_sub(self.start.elapsed())
            .ok_or(Error::Budget)
    }
    fn consume(&mut self, n: u64) -> Result<()> {
        self.check()?;
        self.remaining = self.remaining.checked_sub(n).ok_or(Error::Budget)?;
        Ok(())
    }
}
struct Counted<'a, 'b, R> {
    source: R,
    work: &'a mut Work<'b>,
}
impl<R: Read> Read for Counted<'_, '_, io::Take<R>> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.work.check().map_err(io::Error::other)?;
        let size = b.len().min(65536).min(self.work.remaining as usize);
        if size == 0 && !b.is_empty() && self.source.limit() != 0 {
            return Err(io::Error::other(Error::Budget));
        }
        let n = self.source.read(&mut b[..size])?;
        self.work.consume(n as u64).map_err(io::Error::other)?;
        Ok(n)
    }
}
struct Documents {
    packet: upload_witness::Packet,
}
impl Documents {
    fn new(p: &Plan, w: &LocalStore) -> Result<Self> {
        p.validate()?;
        if Place::local(w, p.witness.generation).map_err(|_| Error::InvalidPlan)? != p.witness
            || w.supports_retirement()
        {
            return Err(Error::InvalidPlan);
        }
        let id = p.id()?;
        let scope = digest(&(&p.target.instance, &p.operation))?;
        let intent_bytes =
            encode(&serde_json::json!({"schema":"tig.s3-upload-intent.design-1","plan":p}))?;
        let done_bytes = encode(
            &serde_json::json!({"schema":"tig.s3-upload-completion.design-1","plan":id,"object":p.publication.object,"target":p.target}),
        )?;
        let request = |role: &str, b: &[u8]| -> Result<Request> {
            Ok(Request {
                operation: Key::parse(&format!("s3-{role}-{}", scope.as_str()))
                    .map_err(store_error)?,
                owner: p.owner.clone(),
                object: Object::from_bytes(b),
                required: AckLevel::ProcessRestart,
            })
        };
        let intent = request("intent", &intent_bytes)?;
        let done = request("done", &done_bytes)?;
        let packet = upload_witness::Packet::new(
            w,
            &id,
            &p.owner,
            [
                upload_witness::Document {
                    operation: intent.operation,
                    bytes: intent_bytes,
                },
                upload_witness::Document {
                    operation: done.operation,
                    bytes: done_bytes,
                },
            ],
        )
        .map_err(witness_error)?;
        Ok(Self { packet })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    Absent,
    Verified,
}
#[derive(Debug, Serialize)]
pub struct Status {
    pub schema: &'static str,
    pub plan: Digest,
    pub historical_completion: bool,
    pub intent_recorded: bool,
    pub accounting_settled: bool,
    pub requires_recovery: bool,
    pub observed: Observation,
    pub retained_custody: bool,
    pub removal_authority: bool,
}
pub fn status(
    witness: &LocalStore,
    ledger: &mut CapacityLedger,
    job: &JournalRef,
    client: &Client,
    p: &Plan,
) -> Result<Status> {
    if ledger.identity() != &p.capacity {
        return Err(Error::InvalidPlan);
    }
    client.check(p)?;
    let docs = Documents::new(p, witness)?;
    let state = docs
        .packet
        .inspect(witness, ledger, job)
        .map_err(witness_error)?;
    let done = state.completion;
    let mut cancel = || false;
    let mut work = Work::new(&p.budget, &mut cancel);
    let observed = client.observe(&mut work)?;
    Ok(Status {
        schema: "tig.s3-upload-status.design-1",
        plan: p.id()?,
        historical_completion: done,
        intent_recorded: state.intent,
        accounting_settled: state.settled,
        requires_recovery: done && observed == Observation::Absent,
        observed,
        retained_custody: false,
        removal_authority: false,
    })
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema: &'static str,
    pub plan: Digest,
    pub object: Object,
    pub payload_io_bytes: u64,
    pub retained_custody: bool,
    pub removal_authority: bool,
}
pub struct Execution<'a> {
    pub source: &'a LocalStore,
    pub witness: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub job: &'a JournalRef,
    pub client: &'a Client,
    pub context: &'a Digest,
}
impl Execution<'_> {
    pub fn apply(&mut self, p: &Plan) -> Result<Receipt> {
        self.apply_cancellable(p, &mut || false)
    }
    pub fn apply_cancellable(
        &mut self,
        p: &Plan,
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Receipt> {
        p.validate()?;
        self.client.check(p)?;
        if self.ledger.identity() != &p.capacity
            || self.context != &p.context
            || Place::local(self.source, p.source.generation).map_err(|_| Error::InvalidPlan)?
                != p.source
        {
            return Err(Error::InvalidPlan);
        }
        let mut work = Work::new(&p.budget, cancel);
        work.check()?;
        let docs = Documents::new(p, self.witness)?;
        let mut source = self
            .source
            .completed_reader(&p.publication)
            .map_err(store_error)?
            .ok_or(Error::NeedsRecovery)?
            .1;
        let pending = docs
            .packet
            .begin(self.witness, self.ledger, self.job)
            .map_err(witness_error)?;
        let done = pending.state.completion;
        work.check()?;
        let observed = self.client.observe(&mut work)?;
        if done && observed == Observation::Absent {
            return Err(Error::MissingAcknowledgedRemote);
        }
        if observed == Observation::Absent {
            self.client.upload(&mut work, &mut source)?;
            if self.client.observe(&mut work)? != Observation::Verified {
                return Err(Error::Integrity);
            }
        }
        work.check()?;
        pending.complete().map_err(witness_error)?;
        Ok(Receipt {
            schema: "tig.s3-upload-receipt.design-1",
            plan: p.id()?,
            object: p.publication.object.clone(),
            payload_io_bytes: p.budget.transfer_bytes - work.remaining,
            retained_custody: false,
            removal_authority: false,
        })
    }
}
