//! Explicit standalone LFS Basic upload. No discovery, cache mutation or custody.
use crate::{
    cache,
    protocol::{AgentError, UploadBackend},
};
use git_asset_core::{Digest, Object, identity};
use git_asset_store::{
    Key,
    existing::{self, ByteSource, ReadCache},
    ledger::{CapacityLedger, Checkpoint, JournalRef},
    local::LocalStore,
    orchestration::Place,
    upload_witness::{self, Document, Packet},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const MEDIA: &str = "application/vnd.git-lfs+json";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Authority,
    Disabled,
    Budget,
    Busy,
    Conflict,
    Denied,
    Unavailable,
    Integrity,
    MissingAcknowledged,
    Recovery,
    Unsupported,
}
impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::Invalid => "invalid_document",
            Self::Authority => "authority",
            Self::Disabled => "upload_disabled",
            Self::Budget => "budget",
            Self::Busy => "busy",
            Self::Conflict => "operation_conflict",
            Self::Denied => "access_denied",
            Self::Unavailable => "remote_unavailable",
            Self::Integrity => "integrity",
            Self::MissingAcknowledged => "missing_acknowledged_remote",
            Self::Recovery => "needs_recovery",
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
type Result<T> = std::result::Result<T, Error>;
impl From<upload_witness::Error> for Error {
    fn from(e: upload_witness::Error) -> Self {
        match e {
            upload_witness::Error::Budget => Self::Budget,
            upload_witness::Error::Busy => Self::Busy,
            upload_witness::Error::Conflict => Self::Conflict,
            upload_witness::Error::InvalidPlan => Self::Invalid,
            upload_witness::Error::NeedsRecovery => Self::Recovery,
        }
    }
}
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    let b = identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    if b.len() > 16384 {
        return Err(Error::Budget);
    }
    Ok(b)
}
fn digest(v: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(v)?))
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub object_bytes: u64,
    pub total_source_bytes: u64,
    pub objects: u16,
    pub http_requests: u16,
    pub transfer_bytes: u64,
    pub millis: u64,
}
impl Budget {
    fn validate(&self) -> Result<()> {
        if self.object_bytes == 0
            || self.object_bytes > 8 * 1024 * 1024
            || self.total_source_bytes == 0
            || self.total_source_bytes > 64 * 1024 * 1024 * 1024
            || self.objects == 0
            || self.objects > 1024
            || self.http_requests == 0
            || self.http_requests > 16
            || self.transfer_bytes == 0
            || self.transfer_bytes > 128 * 1024 * 1024
            || self.millis == 0
            || self.millis > 600000
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub origin: String,
    pub prefix: String,
    pub headers: Vec<String>,
    /// Private, explicitly enrolled action credentials; never part of plan identity.
    #[serde(default, skip_serializing)]
    pub credentials: BTreeMap<String, String>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remote {
    pub instance: Digest,
    pub generation: u64,
    pub batch_url: String,
    pub headers: BTreeMap<String, String>,
    pub scopes: Vec<Scope>,
    #[serde(default)]
    pub loopback_fixture: bool,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub source_root: PathBuf,
    pub source_identity: Digest,
    pub source_generation: u64,
    pub source_protection: existing::RequiredProtection,
    pub authority_root: Object,
    pub authority_nodes: PathBuf,
    pub authority_checkpoint: Option<git_asset_store::metadata::Checkpoint>,
    pub max_authority_entries: u32,
    pub witness_root: PathBuf,
    pub witness: Place,
    pub ledger_root: PathBuf,
    pub minimum: Checkpoint,
    pub capacity: Digest,
    pub job: JournalRef,
    pub owner: Key,
    pub context: Digest,
    pub operation_scope: Key,
    pub expected_remote: String,
    pub ref_name: String,
    pub remote: Remote,
    pub budget: Budget,
    #[serde(default)]
    pub allow_upload: bool,
}
fn absolute(p: &Path) -> Result<()> {
    if !p.is_absolute()
        || p.components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn uri(s: &str, fixture: bool) -> Result<ureq::http::Uri> {
    if s.len() > 8192 || s.contains('#') {
        return Err(Error::Authority);
    }
    let u: ureq::http::Uri = s.parse().map_err(|_| Error::Authority)?;
    let a = u.authority().ok_or(Error::Authority)?;
    if a.as_str().contains('@')
        || (u.scheme_str() != Some("https")
            && !(fixture
                && u.scheme_str() == Some("http")
                && matches!(u.host(), Some("127.0.0.1" | "[::1]" | "::1"))))
    {
        return Err(Error::Authority);
    }
    path(u.path())?;
    Ok(u)
}
fn origin(u: &ureq::http::Uri) -> String {
    format!(
        "{}://{}",
        u.scheme_str().unwrap_or(""),
        u.authority().map(|a| a.as_str()).unwrap_or("")
    )
}
fn path(raw: &str) -> Result<String> {
    let mut b = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(Error::Authority);
            }
            let s = std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|_| Error::Authority)?;
            b.push(u8::from_str_radix(s, 16).map_err(|_| Error::Authority)?);
            i += 3;
        } else {
            b.push(bytes[i]);
            i += 1;
        }
    }
    let s = String::from_utf8(b).map_err(|_| Error::Authority)?;
    if !s.starts_with('/')
        || s.bytes()
            .any(|c| c < 32 || matches!(c, b'\\' | b'%' | b'?' | b'#' | 127))
        || s.split('/').any(|p| matches!(p, "." | ".."))
    {
        return Err(Error::Authority);
    }
    Ok(s)
}
fn headers(h: &BTreeMap<String, String>) -> Result<()> {
    if h.len() > 32 || h.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>() > 16384 {
        return Err(Error::Budget);
    }
    let mut names = BTreeSet::new();
    for (name, value) in h {
        let n = name.to_ascii_lowercase();
        if !names.insert(n.clone())
            || matches!(
                n.as_str(),
                "host"
                    | "connection"
                    | "content-length"
                    | "transfer-encoding"
                    | "proxy-authorization"
                    | "expect"
                    | "accept-encoding"
                    | "content-encoding"
                    | "cookie"
                    | "if-none-match"
            )
            || ureq::http::HeaderName::from_bytes(name.as_bytes()).is_err()
            || ureq::http::HeaderValue::from_str(value).is_err()
        {
            return Err(Error::Authority);
        }
    }
    Ok(())
}
fn fixed_header(h: &BTreeMap<String, String>, name: &str, expected: &str) -> Result<()> {
    if h.iter()
        .any(|(k, v)| k.eq_ignore_ascii_case(name) && !v.eq_ignore_ascii_case(expected))
    {
        return Err(Error::Authority);
    }
    Ok(())
}
// Uppercase RFC3339, second precision, as specified by the LFS Batch contract.
// Leap seconds and pre-epoch instants are unsupported, rather than guessed.
fn expires_at(s: &str) -> Result<SystemTime> {
    let b = s.as_bytes();
    if !matches!(b.len(), 20 | 25)
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return Err(Error::Invalid);
    }
    let number = |start: usize, end: usize| -> Result<i64> {
        let part = &b[start..end];
        if !part.iter().all(u8::is_ascii_digit) {
            return Err(Error::Invalid);
        }
        Ok(part.iter().fold(0, |v, c| v * 10 + (c - b'0') as i64))
    };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => return Err(Error::Invalid),
    };
    if year < 1970 || day < 1 || day > days || hour > 23 || minute > 59 || second > 59 {
        return Err(Error::Invalid);
    }
    let offset = if b.len() == 20 {
        if b[19] != b'Z' {
            return Err(Error::Invalid);
        }
        0
    } else {
        if !matches!(b[19], b'+' | b'-') || b[22] != b':' {
            return Err(Error::Invalid);
        }
        let hours = number(20, 22)?;
        let minutes = number(23, 25)?;
        if hours > 23 || minutes > 59 {
            return Err(Error::Invalid);
        }
        (hours * 3600 + minutes * 60) * if b[19] == b'+' { 1 } else { -1 }
    };
    let mut total_days = 0;
    for y in 1970..year {
        total_days += if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
    }
    for m in 1..month {
        total_days += match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            _ => {
                if leap {
                    29
                } else {
                    28
                }
            }
        };
    }
    let seconds = (total_days + day - 1) * 86400 + hour * 3600 + minute * 60 + second - offset;
    UNIX_EPOCH
        .checked_add(Duration::from_secs(
            u64::try_from(seconds).map_err(|_| Error::Invalid)?,
        ))
        .ok_or(Error::Invalid)
}
impl Profile {
    pub fn from_json(b: &[u8]) -> Result<Self> {
        if b.len() > 65536 {
            return Err(Error::Budget);
        }
        let p: Self = serde_json::from_value(identity::parse_json(b).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
        p.validate()?;
        Ok(p)
    }
    fn validate(&self) -> Result<()> {
        if self.schema != "tig.lfs-upload.design-1"
            || self.source_generation == 0
            || self.remote.generation == 0
            || self.witness.instance == self.source_identity
            || self.max_authority_entries == 0
            || self.max_authority_entries > 2_000_000
            || self.authority_root.bytes > 65536
            || self.expected_remote.is_empty()
            || self.expected_remote.len() > 2048
            || self.expected_remote.bytes().any(|c| c < 32)
            || !self.ref_name.starts_with("refs/")
            || self.ref_name.len() > 1024
            || self
                .ref_name
                .bytes()
                .any(|c| !c.is_ascii_alphanumeric() && !b"/._-".contains(&c))
            || self.ref_name.contains("..")
        {
            return Err(Error::Invalid);
        }
        for p in [
            &self.source_root,
            &self.authority_nodes,
            &self.witness_root,
            &self.ledger_root,
        ] {
            absolute(p)?;
        }
        self.budget.validate()?;
        headers(&self.remote.headers)?;
        fixed_header(&self.remote.headers, "content-type", MEDIA)?;
        fixed_header(&self.remote.headers, "accept", MEDIA)?;
        let u = uri(&self.remote.batch_url, self.remote.loopback_fixture)?;
        if u.query().is_some() {
            return Err(Error::Authority);
        }
        if self.remote.scopes.is_empty() || self.remote.scopes.len() > 16 {
            return Err(Error::Budget);
        }
        for s in &self.remote.scopes {
            let u = uri(&s.origin, self.remote.loopback_fixture)?;
            if origin(&u) != s.origin
                || u.path() != "/"
                || u.query().is_some()
                || !s.prefix.ends_with('/')
                || path(&s.prefix)? != s.prefix
                || s.headers.len() > 32
            {
                return Err(Error::Authority);
            }
            let names = s
                .headers
                .iter()
                .map(|n| n.to_ascii_lowercase())
                .collect::<BTreeSet<_>>();
            if names.len() != s.headers.len() {
                return Err(Error::Authority);
            }
            for n in &names {
                headers(&BTreeMap::from([(n.clone(), "permitted".into())]))?;
            }
            headers(&s.credentials)?;
            if s.credentials
                .keys()
                .any(|k| !names.contains(&k.to_ascii_lowercase()))
            {
                return Err(Error::Authority);
            }
        }
        Ok(())
    }
    fn target(&self) -> Result<Digest> {
        let u = uri(&self.remote.batch_url, self.remote.loopback_fixture)?;
        digest(&(
            &self.remote.instance,
            self.remote.generation,
            origin(&u),
            u.path(),
            &self.remote.scopes,
            &self.expected_remote,
            &self.ref_name,
        ))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    pub object: Object,
    pub operation: Key,
    pub owner: Key,
    pub context: Digest,
    pub source: Digest,
    pub source_generation: u64,
    pub authority: Object,
    pub target: Digest,
    pub witness: Place,
    pub capacity: Digest,
    pub budget: Budget,
}
impl Plan {
    pub fn id(&self) -> Result<Digest> {
        if self.schema != "tig.lfs-upload-plan.design-1"
            || self.object.bytes > self.budget.object_bytes
            || self.source_generation == 0
        {
            return Err(Error::Invalid);
        }
        self.budget.validate()?;
        digest(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(b: &[u8]) -> Result<Self> {
        if b.len() > 16384 {
            return Err(Error::Budget);
        }
        let p: Self = serde_json::from_value(identity::parse_json(b).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
        if p.to_json()? != b {
            return Err(Error::Invalid);
        }
        Ok(p)
    }
    fn packet(&self, w: &LocalStore) -> Result<Packet> {
        if Place::local(w, self.witness.generation).map_err(|_| Error::Authority)? != self.witness {
            return Err(Error::Authority);
        }
        let id = self.id()?;
        let scope = digest(&(&self.target, &self.operation))?;
        Ok(Packet::new(
            w,
            &id,
            &self.owner,
            [
                Document {
                    operation: Key::parse(&format!("lfs-intent-{}", scope.as_str()))
                        .map_err(|_| Error::Invalid)?,
                    bytes: encode(&json!({"schema":"tig.lfs-upload-intent.design-1","plan":self}))?,
                },
                Document {
                    operation: Key::parse(&format!("lfs-done-{}", scope.as_str()))
                        .map_err(|_| Error::Invalid)?,
                    bytes: encode(
                        &json!({"schema":"tig.lfs-upload-completion.design-1","plan":id,"object":self.object,"target":self.target}),
                    )?,
                },
            ],
        )?)
    }
    pub fn required_peak(&self, w: &LocalStore) -> Result<u64> {
        Ok(self.packet(w)?.required_peak()?)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    href: String,
    #[serde(default)]
    header: BTreeMap<String, String>,
    expires_in: Option<i64>,
    expires_at: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Actions {
    upload: Option<Action>,
    download: Option<Action>,
    verify: Option<Action>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectError {
    code: u16,
    message: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    oid: Digest,
    size: u64,
    authenticated: Option<bool>,
    actions: Option<Actions>,
    error: Option<ObjectError>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    transfer: Option<String>,
    objects: Vec<Row>,
    hash_algo: Option<String>,
}
struct Work {
    start: Instant,
    limit: Duration,
    left: u64,
    requests: u16,
    original: u64,
}
impl Work {
    fn new(b: &Budget) -> Self {
        Self {
            start: Instant::now(),
            limit: Duration::from_millis(b.millis),
            left: b.transfer_bytes,
            requests: b.http_requests,
            original: b.transfer_bytes,
        }
    }
    fn check(&self) -> Result<()> {
        if self.start.elapsed() >= self.limit {
            return Err(Error::Budget);
        }
        Ok(())
    }
    fn call(&mut self) -> Result<Duration> {
        self.check()?;
        self.requests = self.requests.checked_sub(1).ok_or(Error::Budget)?;
        self.limit
            .checked_sub(self.start.elapsed())
            .ok_or(Error::Budget)
    }
    fn bytes(&mut self, n: u64) -> Result<()> {
        self.check()?;
        self.left = self.left.checked_sub(n).ok_or(Error::Budget)?;
        Ok(())
    }
}
struct Counted<'a, R> {
    reader: R,
    work: &'a mut Work,
}
impl<R: Read> Read for Counted<'_, io::Take<R>> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.work.check().map_err(io::Error::other)?;
        let n = b.len().min(65536).min(self.work.left as usize);
        if n == 0 && self.reader.limit() != 0 && !b.is_empty() {
            return Err(io::Error::other(Error::Budget));
        }
        let n = self.reader.read(&mut b[..n])?;
        self.work.bytes(n as u64).map_err(io::Error::other)?;
        Ok(n)
    }
}
pub struct Backend {
    profile: Profile,
    cache: ReadCache,
    agent: ureq::Agent,
    requests: u16,
}
impl Backend {
    pub fn new(profile: Profile) -> Result<Self> {
        profile.validate()?;
        let cache = ReadCache::new(existing::Profile {
            places: vec![existing::Place {
                root: profile.source_root.clone(),
                layout: existing::Layout::GitLfs,
            }],
            required: profile.source_protection,
            limits: existing::Limits {
                object_bytes: profile.budget.object_bytes,
                total_bytes: profile.budget.total_source_bytes,
                requests: profile.budget.objects as u32,
                verify_millis: profile.budget.millis.min(300000),
                live_readers: 1,
            },
        })
        .map_err(|_| Error::Invalid)?;
        let agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .max_idle_connections(0)
            .http_status_as_error(false)
            .max_response_header_size(16384)
            .input_buffer_size(65536)
            .output_buffer_size(65536)
            .build()
            .into();
        Ok(Self {
            profile,
            cache,
            agent,
            requests: 0,
        })
    }
    fn authorize(&self, o: &Object) -> Result<()> {
        cache::authorize_oid(
            &self.profile.authority_root,
            &self.profile.authority_nodes,
            self.profile.authority_checkpoint.as_ref(),
            self.profile.max_authority_entries,
            o,
        )
        .map_err(|_| Error::Authority)
    }
    pub fn plan(&self, object: Object) -> Result<Plan> {
        self.authorize(&object)?;
        let p = &self.profile;
        let operation = Key::parse(&format!(
            "upload-{}",
            digest(&(
                &p.operation_scope,
                &p.context,
                &p.source_identity,
                p.source_generation,
                &p.authority_root,
                p.target()?,
                &object
            ))?
            .as_str()
        ))
        .map_err(|_| Error::Invalid)?;
        let plan = Plan {
            schema: "tig.lfs-upload-plan.design-1".into(),
            object,
            operation,
            owner: p.owner.clone(),
            context: p.context.clone(),
            source: p.source_identity.clone(),
            source_generation: p.source_generation,
            authority: p.authority_root.clone(),
            target: p.target()?,
            witness: p.witness.clone(),
            capacity: p.capacity.clone(),
            budget: p.budget.clone(),
        };
        plan.id()?;
        Ok(plan)
    }
    fn admit(&self, p: &Plan) -> Result<()> {
        if self.plan(p.object.clone())?.to_json()? != p.to_json()? {
            return Err(Error::Authority);
        }
        Ok(())
    }
    fn local(&self) -> Result<(LocalStore, CapacityLedger)> {
        let w = LocalStore::open(&self.profile.witness_root).map_err(|_| Error::Recovery)?;
        if Place::local(&w, self.profile.witness.generation).map_err(|_| Error::Authority)?
            != self.profile.witness
        {
            return Err(Error::Authority);
        }
        let l = CapacityLedger::open(&self.profile.ledger_root, &self.profile.minimum)
            .map_err(|_| Error::Recovery)?;
        if l.identity() != &self.profile.capacity {
            return Err(Error::Authority);
        }
        Ok((w, l))
    }
    pub fn peak(&self, p: &Plan) -> Result<u64> {
        self.admit(p)?;
        let (w, _) = self.local()?;
        p.required_peak(&w)
    }
    fn action(&self, a: &Action, work: &Work) -> Result<BTreeMap<String, String>> {
        let u = uri(&a.href, self.profile.remote.loopback_fixture)?;
        let p = path(u.path())?;
        headers(&a.header)?;
        let allowed = self
            .profile
            .remote
            .scopes
            .iter()
            .filter(|s| {
                s.origin == origin(&u)
                    && p.starts_with(&s.prefix)
                    && a.header
                        .keys()
                        .all(|k| s.headers.iter().any(|n| n.eq_ignore_ascii_case(k)))
            })
            .collect::<Vec<_>>();
        // Ambiguous credential scopes must not choose the first matching entry.
        if allowed.len() != 1 {
            return Err(Error::Authority);
        }
        if a.expires_in.is_some_and(|n| {
            n <= 0 || n > i32::MAX as i64 || work.start.elapsed() >= Duration::from_secs(n as u64)
        }) {
            return Err(Error::Denied);
        }
        if a.expires_in.is_none()
            && let Some(expiry) = &a.expires_at
            && SystemTime::now() >= expires_at(expiry)?
        {
            return Err(Error::Denied);
        }
        let mut merged = a
            .header
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
            .collect::<BTreeMap<_, _>>();
        for (k, v) in &allowed[0].credentials {
            let k = k.to_ascii_lowercase();
            if merged.get(&k).is_some_and(|current| current != v) {
                return Err(Error::Authority);
            }
            merged.insert(k, v.clone());
        }
        headers(&merged)?;
        Ok(merged)
    }
    fn response(code: u16) -> Result<()> {
        match code {
            200..=299 if code != 206 => Ok(()),
            401 | 403 => Err(Error::Denied),
            409 => Err(Error::Conflict),
            _ => Err(Error::Unavailable),
        }
    }
    fn body(mut r: ureq::http::Response<ureq::Body>, work: &mut Work) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut reader = r.body_mut().as_reader().take(65537);
        let mut b = [0; 4096];
        loop {
            work.check()?;
            let n = reader.read(&mut b).map_err(|_| Error::Unavailable)?;
            if n == 0 {
                break;
            }
            work.bytes(n as u64)?;
            bytes.extend_from_slice(&b[..n]);
            if bytes.len() > 65536 {
                return Err(Error::Budget);
            }
        }
        Ok(bytes)
    }
    fn batch(&self, operation: &str, o: &Object, work: &mut Work) -> Result<Option<Actions>> {
        let body = encode(
            &json!({"operation":operation,"transfers":["basic"],"hash_algo":"sha256","ref":{"name":self.profile.ref_name},"objects":[{"oid":o.sha256,"size":o.bytes}]}),
        )?;
        work.bytes(body.len() as u64)?;
        let time = work.call()?;
        let mut request = self.agent.post(&self.profile.remote.batch_url);
        for (k, v) in &self.profile.remote.headers {
            request = request.header(k.as_str(), v.as_str());
        }
        let result = request
            .header("content-type", MEDIA)
            .header("accept", MEDIA)
            .config()
            .timeout_global(Some(time))
            .build()
            .send(body.as_slice());
        work.check()?;
        let result = result.map_err(|_| Error::Unavailable)?;
        Self::response(result.status().as_u16())?;
        let batch: Batch = serde_json::from_value(
            identity::parse_json(&Self::body(result, work)?).map_err(|_| Error::Invalid)?,
        )
        .map_err(|_| Error::Invalid)?;
        if batch.transfer.as_ref().is_some_and(|s| s != "basic")
            || batch.hash_algo.as_ref().is_some_and(|s| s != "sha256")
        {
            return Err(Error::Unsupported);
        }
        if batch.objects.len() != 1 {
            return Err(Error::Integrity);
        }
        let row = batch.objects.into_iter().next().ok_or(Error::Integrity)?;
        if row.oid != o.sha256 || row.size != o.bytes {
            return Err(Error::Integrity);
        }
        let _ = row.authenticated;
        if let Some(e) = row.error {
            let _ = e.message;
            if row.actions.is_some() {
                return Err(Error::Invalid);
            }
            return match e.code {
                404 => Ok(None),
                401 | 403 => Err(Error::Denied),
                _ => Err(Error::Unavailable),
            };
        }
        Ok(Some(row.actions.unwrap_or_default()))
    }
    fn readback(&self, o: &Object, work: &mut Work) -> Result<bool> {
        let Some(actions) = self.batch("download", o, work)? else {
            return Ok(false);
        };
        if actions.upload.is_some() || actions.verify.is_some() {
            return Err(Error::Unsupported);
        }
        let a = actions.download.ok_or(Error::Integrity)?;
        let action_headers = self.action(&a, work)?;
        if work.left < o.bytes {
            return Err(Error::Budget);
        }
        let time = work.call()?;
        let mut req = self.agent.get(&a.href);
        for (k, v) in &action_headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let response = req
            .header("accept-encoding", "identity")
            .config()
            .timeout_global(Some(time))
            .build()
            .call();
        work.check()?;
        let mut response = response.map_err(|_| Error::Unavailable)?;
        if response.status().as_u16() == 404 {
            return Ok(false);
        }
        if response.status() != 200 {
            Self::response(response.status().as_u16())?;
            return Err(Error::Unavailable);
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|v| v.as_bytes() != b"identity")
        {
            return Err(Error::Integrity);
        }
        if response
            .headers()
            .get("content-length")
            .is_some_and(|v| v.to_str().ok().and_then(|s| s.parse::<u64>().ok()) != Some(o.bytes))
        {
            return Err(Error::Integrity);
        }
        let mut reader = response.body_mut().as_reader();
        let mut total = 0;
        let mut hash = Sha256::new();
        let mut b = [0; 65536];
        loop {
            work.check()?;
            let count = ((o.bytes + 1 - total).min(65536)) as usize;
            let n = reader
                .read(&mut b[..count])
                .map_err(|_| Error::Unavailable)?;
            if n == 0 {
                break;
            }
            work.bytes(n as u64)?;
            total += n as u64;
            if total > o.bytes {
                return Err(Error::Integrity);
            }
            hash.update(&b[..n]);
        }
        let result = hash
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if total != o.bytes || result != o.sha256.as_str() {
            return Err(Error::Integrity);
        }
        Ok(true)
    }
    fn transfer(&self, o: &Object, source: &mut dyn Read, work: &mut Work) -> Result<()> {
        let actions = self.batch("upload", o, work)?.ok_or(Error::Unavailable)?;
        if actions.download.is_some() {
            return Err(Error::Unsupported);
        }
        if let Some(a) = actions.upload {
            let action_headers = self.action(&a, work)?;
            fixed_header(&action_headers, "content-type", "application/octet-stream")?;
            if work.left < o.bytes {
                return Err(Error::Budget);
            }
            let time = work.call()?;
            let mut req = self.agent.put(&a.href);
            for (k, v) in &action_headers {
                req = req.header(k.as_str(), v.as_str());
            }
            let mut reader = Counted {
                reader: source.take(o.bytes),
                work,
            };
            let result = req
                .header("if-none-match", "*")
                .header("content-type", "application/octet-stream")
                .header("content-length", o.bytes.to_string())
                .config()
                .timeout_global(Some(time))
                .build()
                .send(ureq::SendBody::from_reader(&mut reader));
            reader.work.check()?;
            let result = result.map_err(|_| Error::Unavailable)?;
            if result.status().as_u16() != 412 {
                Self::response(result.status().as_u16())?;
            }
        }
        if let Some(a) = actions.verify {
            let action_headers = self.action(&a, work)?;
            fixed_header(&action_headers, "content-type", MEDIA)?;
            fixed_header(&action_headers, "accept", MEDIA)?;
            let b = encode(&json!({"oid":o.sha256,"size":o.bytes}))?;
            work.bytes(b.len() as u64)?;
            let time = work.call()?;
            let mut req = self.agent.post(&a.href);
            for (k, v) in &action_headers {
                req = req.header(k.as_str(), v.as_str());
            }
            let response = req
                .header("content-type", MEDIA)
                .header("accept", MEDIA)
                .config()
                .timeout_global(Some(time))
                .build()
                .send(b.as_slice());
            work.check()?;
            let response = response.map_err(|_| Error::Unavailable)?;
            if response.status() != 200 {
                Self::response(response.status().as_u16())?;
                return Err(Error::Unavailable);
            }
        }
        Ok(())
    }
    pub fn apply(&mut self, p: &Plan, input_path: &Path) -> Result<Value> {
        self.admit(p)?;
        if !self.profile.allow_upload {
            return Err(Error::Disabled);
        }
        let expected =
            existing::Layout::GitLfs.object_path(&self.profile.source_root, &p.object.sha256);
        if input_path != expected {
            return Err(Error::Authority);
        }
        if self.requests >= self.profile.budget.objects {
            return Err(Error::Budget);
        }
        self.requests += 1;
        let mut source = self.cache.open_verified(&p.object).map_err(|e| match e {
            existing::Error::Busy => Error::Busy,
            existing::Error::Budget => Error::Budget,
            existing::Error::Integrity => Error::Integrity,
            _ => Error::Authority,
        })?;
        let (w, mut ledger) = self.local()?;
        let packet = p.packet(&w)?;
        let pending = packet.begin(&w, &mut ledger, &self.profile.job)?;
        let mut work = Work::new(&p.budget);
        let present = self.readback(&p.object, &mut work)?;
        if pending.state.completion && !present {
            return Err(Error::MissingAcknowledged);
        }
        // Even reusable bytes need this operation's upload admission/optional
        // verify notification. A prior failed verify must not be skipped merely
        // because a download can already read the content store's bytes.
        if !pending.state.completion {
            self.transfer(&p.object, &mut source, &mut work)?;
            if !self.readback(&p.object, &mut work)? {
                return Err(Error::Integrity);
            }
        }
        pending.complete()?;
        Ok(
            json!({"schema":"tig.lfs-upload-receipt.design-1","plan":p.id()?,"object":p.object,"payload_io_bytes":work.original-work.left,"retained_custody":false,"removal_authority":false}),
        )
    }
    pub fn status(&self, p: &Plan) -> Result<Value> {
        self.admit(p)?;
        let (w, mut l) = self.local()?;
        let state = p.packet(&w)?.inspect(&w, &mut l, &self.profile.job)?;
        let mut work = Work::new(&p.budget);
        let present = self.readback(&p.object, &mut work)?;
        Ok(
            json!({"schema":"tig.lfs-upload-status.design-1","plan":p.id()?,"historical_completion":state.completion,"intent_recorded":state.intent,"accounting_settled":state.settled,"verified_present":present,"requires_recovery":state.completion&&!present,"retained_custody":false,"removal_authority":false}),
        )
    }
    pub fn source_path(&self, o: &Object) -> PathBuf {
        existing::Layout::GitLfs.object_path(&self.profile.source_root, &o.sha256)
    }
}
fn agent(e: Error) -> AgentError {
    match e {
        Error::Authority | Error::Disabled => AgentError::Authority,
        Error::Budget => AgentError::Limit,
        Error::Busy => AgentError::Busy,
        Error::Integrity => AgentError::Integrity,
        Error::Unsupported => AgentError::Unsupported,
        _ => AgentError::Unavailable,
    }
}
impl UploadBackend for Backend {
    fn initialize(&mut self, remote: &str) -> crate::protocol::Result<()> {
        if remote != self.profile.expected_remote || !self.profile.allow_upload {
            return Err(AgentError::Authority);
        }
        self.local().map_err(agent)?;
        Ok(())
    }
    fn upload(&mut self, o: &Object, path: &Path) -> crate::protocol::Result<()> {
        let p = self.plan(o.clone()).map_err(agent)?;
        self.apply(&p, path).map_err(agent)?;
        Ok(())
    }
}
