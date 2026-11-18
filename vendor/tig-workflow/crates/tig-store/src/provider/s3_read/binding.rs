use super::*;
/// Trusted provider/version/URL binding. Presigned URLs are secrets; no serde/Debug.
/// The caller admits an S3 implementation with immutable non-null version semantics.
pub struct Config {
    pub(super) place: Digest,
    pub(super) object: Object,
    pub(super) version: Version,
    pub(super) head: Uri,
    pub(super) get: Uri,
    pub(super) limits: Limits,
    roots: Vec<Certificate<'static>>,
}
fn decode(s: &str) -> Result<Vec<u8>, SourceError> {
    let mut result = Vec::new();
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let a = (bytes.next().ok_or(SourceError::Invalid)? as char)
                .to_digit(16)
                .ok_or(SourceError::Invalid)?;
            let c = (bytes.next().ok_or(SourceError::Invalid)? as char)
                .to_digit(16)
                .ok_or(SourceError::Invalid)?;
            result.push((a * 16 + c) as u8);
        } else {
            result.push(b)
        }
    }
    Ok(result)
}
fn target(s: &str, version: &Version, loopback: bool) -> Result<Uri, SourceError> {
    if s.len() > 8192 || s.contains('#') {
        return Err(SourceError::Invalid);
    }
    let uri: Uri = s.parse().map_err(|_| SourceError::Invalid)?;
    let scheme = uri.scheme_str().ok_or(SourceError::Invalid)?;
    let authority = uri.authority().ok_or(SourceError::Invalid)?;
    let host = uri.host().ok_or(SourceError::Invalid)?;
    if authority.as_str().contains('@') || host.contains(['%', '\\']) || uri.path().is_empty() {
        return Err(SourceError::Invalid);
    }
    let plain = scheme == "http"
        && loopback
        && host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|v| v.is_loopback());
    if scheme != "https" && !plain {
        return Err(SourceError::Unsupported);
    }
    if uri.port_u16() == Some(0) {
        return Err(SourceError::Invalid);
    }
    if uri.port_u16().is_none()
        && authority.as_str() != host
        && authority.as_str() != format!("[{host}]")
    {
        return Err(SourceError::Invalid);
    }
    let mut found = false;
    for pair in uri.query().ok_or(SourceError::Invalid)?.split('&') {
        let (k, v) = pair.split_once('=').ok_or(SourceError::Invalid)?;
        if decode(k)? == b"versionId" {
            if found || decode(v)? != version.as_bytes() {
                return Err(SourceError::Invalid);
            }
            found = true;
        }
    }
    if !found {
        return Err(SourceError::Invalid);
    }
    Ok(uri)
}
impl Config {
    pub fn https(
        place: Digest,
        object: Object,
        version: &str,
        head_url: &str,
        get_url: &str,
        limits: Limits,
    ) -> Result<Self, SourceError> {
        Self::new(place, object, version, head_url, get_url, limits, false)
    }
    /// Only literal loopback HTTP is admitted; use for explicitly owned fixtures.
    pub fn loopback_http(
        place: Digest,
        object: Object,
        version: &str,
        head_url: &str,
        get_url: &str,
        limits: Limits,
    ) -> Result<Self, SourceError> {
        Self::new(place, object, version, head_url, get_url, limits, true)
    }
    fn new(
        place: Digest,
        object: Object,
        version: &str,
        head_url: &str,
        get_url: &str,
        limits: Limits,
        loopback: bool,
    ) -> Result<Self, SourceError> {
        if version == "null" || version.bytes().any(|b| b <= 32 || b >= 127) {
            return Err(SourceError::Unsupported);
        }
        let version = Version::new(version.as_bytes())?;
        if limits.max_object_bytes > 9_007_199_254_740_991
            || limits.max_requests == 0
            || limits.max_requests > 4096
            || !(1..=16).contains(&limits.max_streams)
            || limits.max_total_bytes == 0
            || limits.request_timeout.is_zero()
            || limits.request_timeout > limits.session_timeout
            || limits.session_timeout > Duration::from_secs(86_400)
        {
            return Err(SourceError::Invalid);
        }
        let head = target(head_url, &version, loopback)?;
        let get = target(get_url, &version, loopback)?;
        if head.scheme() != get.scheme()
            || head.authority() != get.authority()
            || head.path() != get.path()
        {
            return Err(SourceError::Invalid);
        }
        Ok(Self {
            place,
            object,
            version,
            head,
            get,
            limits,
            roots: vec![],
        })
    }
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
            )
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
