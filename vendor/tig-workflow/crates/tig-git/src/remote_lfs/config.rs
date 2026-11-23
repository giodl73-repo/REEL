use super::*;
use std::{collections::BTreeMap, net::IpAddr};
use ureq::{
    http::{HeaderValue, Uri},
    tls::{Certificate, RootCerts, TlsConfig},
};

#[derive(Clone, Debug)]
pub struct Limits {
    pub max_object_bytes: u64,
    pub max_requests: u32,
    pub max_streams: usize,
    pub max_total_bytes: u64,
    pub metadata_bytes: usize,
    pub request_timeout: Duration,
    pub session_timeout: Duration,
    pub action_ttl: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_object_bytes: 64 << 30,
            max_requests: 128,
            max_streams: 4,
            max_total_bytes: 256 << 30,
            metadata_bytes: 65_536,
            request_timeout: Duration::from_secs(30),
            session_timeout: Duration::from_secs(300),
            action_ttl: Duration::from_secs(30),
        }
    }
}
impl Limits {
    fn validate(&self) -> Result<(), SourceError> {
        if self.max_object_bytes > 9_007_199_254_740_991
            || self.max_requests == 0
            || self.max_requests > 4096
            || !(1..=16).contains(&self.max_streams)
            || self.max_total_bytes == 0
            || !(1..=65_536).contains(&self.metadata_bytes)
            || self.request_timeout.is_zero()
            || self.request_timeout > self.session_timeout
            || self.session_timeout > Duration::from_secs(86_400)
            || self.action_ttl.is_zero()
            || self.action_ttl > Duration::from_secs(300)
        {
            return Err(SourceError::Invalid);
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Secret(pub(super) HeaderValue);
impl Secret {
    pub fn new(value: &str) -> Result<Self, SourceError> {
        if value.is_empty() {
            return Err(SourceError::Invalid);
        }
        if value.len() > 4096 {
            return Err(SourceError::Budget);
        }
        let mut value = HeaderValue::from_str(value).map_err(|_| SourceError::Invalid)?;
        value.set_sensitive(true);
        Ok(Self(value))
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Origin(String);
pub(super) fn target(value: &str, loopback: bool) -> Result<(Uri, Origin), SourceError> {
    if value.len() > 8192 || value.contains('#') {
        return Err(SourceError::Invalid);
    }
    let uri: Uri = value.parse().map_err(|_| SourceError::Invalid)?;
    let scheme = uri.scheme_str().ok_or(SourceError::Invalid)?;
    let authority = uri.authority().ok_or(SourceError::Invalid)?;
    let host = uri.host().ok_or(SourceError::Invalid)?;
    if authority.as_str().contains('@') || host.contains('%') || host.contains('\\') {
        return Err(SourceError::Invalid);
    }
    let plain = scheme == "http"
        && loopback
        && host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|a| a.is_loopback());
    if scheme != "https" && !plain {
        return Err(SourceError::Unsupported);
    }
    let port = uri
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    if uri.port_u16().is_none()
        && authority.as_str() != host
        && authority.as_str() != format!("[{host}]")
    {
        return Err(SourceError::Invalid);
    }
    if port == 0 {
        return Err(SourceError::Invalid);
    }
    let origin = Origin(format!("{scheme}://{}:{port}", host.to_ascii_lowercase()));
    Ok((uri, origin))
}
pub(super) fn allowed_header(name: &str) -> Result<String, SourceError> {
    if name.len() > 128 {
        return Err(SourceError::Budget);
    }
    let name = ureq::http::HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| SourceError::Invalid)?
        .as_str()
        .to_string();
    if [
        "host",
        "content-length",
        "transfer-encoding",
        "connection",
        "proxy-authorization",
        "proxy-connection",
        "cookie",
        "set-cookie",
        "range",
        "accept-encoding",
        "expect",
        "upgrade",
        "te",
        "trailer",
    ]
    .contains(&name.as_str())
    {
        return Err(SourceError::Unsupported);
    }
    Ok(name)
}
/// Trusted explicit endpoint/credential binding. No Debug/serde/discovery or IO.
pub struct Config {
    pub(super) place: Digest,
    pub(super) batch: Uri,
    pub(super) origins: Vec<Origin>,
    pub(super) loopback: bool,
    pub(super) limits: Limits,
    pub(super) batch_auth: Option<Secret>,
    pub(super) download_auth: BTreeMap<Origin, Secret>,
    pub(super) headers: Vec<String>,
    pub(super) roots: Vec<Certificate<'static>>,
}
impl Config {
    pub fn https(place: Digest, batch_endpoint: &str, limits: Limits) -> Result<Self, SourceError> {
        Self::new(place, batch_endpoint, limits, false)
    }
    /// Plain HTTP is admitted only for literal loopback test endpoints, never DNS names.
    pub fn loopback_http(
        place: Digest,
        batch_endpoint: &str,
        limits: Limits,
    ) -> Result<Self, SourceError> {
        Self::new(place, batch_endpoint, limits, true)
    }
    fn new(
        place: Digest,
        batch_endpoint: &str,
        limits: Limits,
        loopback: bool,
    ) -> Result<Self, SourceError> {
        limits.validate()?;
        let (batch, origin) = target(batch_endpoint, loopback)?;
        if batch.query().is_some() {
            return Err(SourceError::Invalid);
        }
        Ok(Self {
            place,
            batch,
            origins: vec![origin.clone()],
            loopback,
            limits,
            batch_auth: None,
            download_auth: BTreeMap::new(),
            headers: vec!["authorization".into()],
            roots: vec![],
        })
    }
    pub fn batch_authorization(mut self, secret: Secret) -> Self {
        self.batch_auth = Some(secret);
        self
    }
    fn origin(&self, url: &str) -> Result<Origin, SourceError> {
        let (uri, origin) = target(url, self.loopback)?;
        if uri.query().is_some() || !matches!(uri.path(), "" | "/") {
            return Err(SourceError::Invalid);
        }
        Ok(origin)
    }
    pub fn allow_download_origin(mut self, origin: &str) -> Result<Self, SourceError> {
        let origin = self.origin(origin)?;
        if !self.origins.contains(&origin) {
            if self.origins.len() >= 16 {
                return Err(SourceError::Budget);
            }
            self.origins.push(origin);
        }
        Ok(self)
    }
    pub fn download_authorization(
        mut self,
        origin: &str,
        secret: Secret,
    ) -> Result<Self, SourceError> {
        let origin = self.origin(origin)?;
        if !self.origins.contains(&origin) {
            return Err(SourceError::Denied);
        }
        self.download_auth.insert(origin, secret);
        Ok(self)
    }
    pub fn allow_action_header(mut self, name: &str) -> Result<Self, SourceError> {
        let name = allowed_header(name)?;
        if !self.headers.contains(&name) {
            if self.headers.len() >= 16 {
                return Err(SourceError::Budget);
            }
            self.headers.push(name);
        }
        Ok(self)
    }
    /// Explicit additional trust binding; verification stays enabled. At most 16 roots.
    pub fn trust_pem(mut self, pem: &[u8]) -> Result<Self, SourceError> {
        if pem.len() > 65_536 || self.roots.len() >= 16 {
            return Err(SourceError::Budget);
        }
        self.roots
            .push(Certificate::from_pem(pem).map_err(|_| SourceError::Invalid)?);
        Ok(self)
    }
    pub fn connect(self) -> Result<Source, SourceError> {
        let mut settings = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .max_response_header_size(32_768)
            .input_buffer_size(16_384)
            .output_buffer_size(16_384)
            .max_idle_connections(4)
            .max_idle_connections_per_host(1)
            .timeout_global(Some(self.limits.request_timeout))
            .timeout_per_read(Some(self.limits.request_timeout));
        if !self.roots.is_empty() {
            settings = settings.tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::new_with_certs(&self.roots))
                    .build(),
            );
        }
        let agent: ureq::Agent = settings.build().into();
        let deadline = Instant::now()
            .checked_add(self.limits.session_timeout)
            .ok_or(SourceError::Invalid)?;
        let budget = Arc::new(Mutex::new(Budget {
            requests: 0,
            streams: 0,
            bytes: 0,
            deadline,
        }));
        Ok(Source {
            config: self,
            agent,
            budget,
        })
    }
}
