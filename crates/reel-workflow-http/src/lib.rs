//! HTTP transport only. Callers own input admission, durable intent, response
//! normalization/publication and uncertain-operation reconciliation.
#[cfg(windows)]
pub mod call;
pub mod proxy;
use reel_workflow::{Error, ErrorKind, Result};
use reqwest::{
    Url,
    blocking::{Client, Request},
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, time::Duration};

const MAX_BYTES: u64 = 128 * 1024 * 1024;
fn error(kind: ErrorKind, code: &str) -> Error {
    Error {
        kind,
        code: code.into(),
    }
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecretHeader {
    pub variable: String,
    pub prefix: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub endpoint: String,
    pub timeout_seconds: u64,
    pub request_byte_limit: u64,
    pub response_byte_limit: u64,
    pub secret_headers: BTreeMap<String, SecretHeader>,
    /// Synthetic/private local API bridges only; never permits arbitrary HTTP.
    #[serde(default)]
    pub allow_loopback_http: bool,
}
#[derive(Serialize)]
pub struct Intent {
    pub schema: &'static str,
    pub operation_id: String,
    pub config_sha256: String,
    pub request_sha256: String,
    pub request_bytes: u64,
}
/// Deliberately not Debug/Serialize: raw provider bytes stay in private evidence.
pub struct Response {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub body_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseHead {
    pub schema: String,
    pub status: u16,
    pub content_type: Option<String>,
    pub request_id: Option<String>,
    pub history_item_id: Option<String>,
}
pub struct Prepared {
    client: Client,
    request: Request,
    intent: Intent,
    response_limit: u64,
}
fn reserved(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "content-length"
            | "content-type"
            | "connection"
            | "transfer-encoding"
            | "proxy-authorization"
            | "x-reel-operation-id"
            | "te"
            | "trailer"
            | "upgrade"
            | "expect"
            | "keep-alive"
            | "proxy-connection"
    )
}
impl Config {
    /// Resolves only named secrets. Neither secret values, raw URLs nor library
    /// diagnostics appear in our errors/receipts. Preparing never sends a call.
    pub fn prepare(
        &self,
        operation_id: &str,
        body: &[u8],
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<Prepared> {
        if self.schema != "reel.workflow-http.v1"
            || !token(operation_id)
            || !(1..=21600).contains(&self.timeout_seconds)
            || !(1..=MAX_BYTES).contains(&self.request_byte_limit)
            || !(1..=MAX_BYTES).contains(&self.response_byte_limit)
            || self.secret_headers.len() > 16
            || self.endpoint.len() > 8192
        {
            return Err(error(ErrorKind::Invalid, "http-config"));
        }
        if body.len() as u64 > self.request_byte_limit {
            return Err(error(ErrorKind::Budget, "http-request-bytes"));
        }
        serde_json::from_slice::<serde_json::Value>(body)
            .map_err(|_| error(ErrorKind::Invalid, "http-request-json"))?;
        let url =
            Url::parse(&self.endpoint).map_err(|_| error(ErrorKind::Invalid, "http-endpoint"))?;
        let local = url
            .host_str()
            .and_then(|h| h.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if (!url.username().is_empty() || url.password().is_some() || url.fragment().is_some())
            || !(url.scheme() == "https"
                || self.allow_loopback_http && url.scheme() == "http" && local)
        {
            return Err(error(ErrorKind::Denied, "http-endpoint-policy"));
        }
        let mut headers = HeaderMap::new();
        for (name, secret) in &self.secret_headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| error(ErrorKind::Invalid, "http-header-name"))?;
            if reserved(name.as_str())
                || headers.contains_key(&name)
                || !token(&secret.variable)
                || secret.prefix.len() > 64
                || secret.prefix.chars().any(|c| c.is_control())
            {
                return Err(error(ErrorKind::Invalid, "http-header-config"));
            }
            let value = resolve(&secret.variable)
                .filter(|s| !s.is_empty() && s.len() <= 16384)
                .ok_or_else(|| error(ErrorKind::Denied, "http-credential-unavailable"))?;
            let mut value = HeaderValue::from_str(&format!("{}{value}", secret.prefix))
                .map_err(|_| error(ErrorKind::Denied, "http-credential-value"))?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert(
            "x-reel-operation-id",
            HeaderValue::from_str(operation_id)
                .map_err(|_| error(ErrorKind::Invalid, "http-operation-id"))?,
        );
        let client = Client::builder()
            .timeout(Duration::from_secs(self.timeout_seconds))
            .connect_timeout(Duration::from_secs(self.timeout_seconds.min(30)))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .http1_only()
            .http1_max_headers(64)
            .build()
            .map_err(|_| error(ErrorKind::Unsupported, "http-client-build"))?;
        let request = client
            .post(url)
            .headers(headers)
            .body(body.to_vec())
            .build()
            .map_err(|_| error(ErrorKind::Invalid, "http-request-build"))?;
        Ok(Prepared {
            client,
            request,
            response_limit: self.response_byte_limit,
            intent: Intent {
                schema: "reel.workflow-http-intent.v1",
                operation_id: operation_id.into(),
                config_sha256: reel_workflow::canonical_digest(self)?,
                request_sha256: reel_workflow::digest(body),
                request_bytes: body.len() as u64,
            },
        })
    }
}
impl Prepared {
    /// The supplied callback MUST durably record this original operation before
    /// returning Ok. An error from that callback prevents network execution.
    /// Every error after it succeeds means reconcile; no retry is attempted.
    pub fn send(self, record_intent: impl FnOnce(&Intent) -> Result<()>) -> Result<Response> {
        self.send_captured(record_intent, |_| Ok(()))
    }
    /// Persist received prefixes through `capture`; this is the integration path
    /// for durable private evidence, including truncated or over-budget replies.
    pub fn send_captured(
        self,
        record_intent: impl FnOnce(&Intent) -> Result<()>,
        capture: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<Response> {
        self.send_observed(record_intent, |_| Ok(()), capture)
    }
    /// Record bounded response correlation headers before reading its body, so
    /// a truncated reply can still be reconciled using the original service ID.
    pub fn send_observed(
        self,
        record_intent: impl FnOnce(&Intent) -> Result<()>,
        record_head: impl FnOnce(&ResponseHead) -> Result<()>,
        mut capture: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<Response> {
        record_intent(&self.intent)?;
        let response = self
            .client
            .execute(self.request)
            .map_err(|_| error(ErrorKind::NeedsRecovery, "http-call-outcome-uncertain"))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .filter(|v| v.len() <= 128)
            .map(str::to_owned);
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|h| h.to_str().ok())
                .filter(|s| {
                    !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| b.is_ascii_graphic())
                })
                .map(str::to_owned)
        };
        record_head(&ResponseHead {
            schema: "reel.workflow-http-head.v1".into(),
            status,
            content_type: content_type.clone(),
            request_id: header("request-id").or_else(|| header("x-request-id")),
            history_item_id: header("history-item-id"),
        })
        .map_err(|_| {
            error(
                ErrorKind::NeedsRecovery,
                "http-response-head-capture-uncertain",
            )
        })?;
        if response
            .content_length()
            .is_some_and(|n| n > self.response_limit)
        {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "http-response-budget-uncertain",
            ));
        }
        let mut body = Vec::new();
        let mut reader = response.take(self.response_limit + 1);
        let mut buffer = [0u8; 65536];
        loop {
            let count = reader
                .read(&mut buffer)
                .map_err(|_| error(ErrorKind::NeedsRecovery, "http-response-read-uncertain"))?;
            if count == 0 {
                break;
            }
            let allowed = count.min((self.response_limit - body.len() as u64) as usize);
            capture(&buffer[..allowed])
                .map_err(|_| error(ErrorKind::NeedsRecovery, "http-response-capture-uncertain"))?;
            body.extend_from_slice(&buffer[..allowed]);
            if count > allowed {
                return Err(error(
                    ErrorKind::NeedsRecovery,
                    "http-response-budget-uncertain",
                ));
            }
        }
        Ok(Response {
            status,
            content_type,
            body_sha256: reel_workflow::digest(&body),
            body,
        })
    }
}
