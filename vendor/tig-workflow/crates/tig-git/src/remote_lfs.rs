//! Explicit bounded LFS Basic downloads. Unprotected ObjectSource, not custody.
use std::{
    io::{self, Read},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tig_core::{Digest, Object};
use tig_store::acquire::{ObjectSource, SourceError};
mod config;
mod response;
pub use config::{Config, Limits, Secret};
struct Budget {
    requests: u32,
    streams: usize,
    bytes: u64,
    deadline: Instant,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    pub requests_charged: u32,
    pub active_streams: usize,
    /// Conservative decoded-byte charge, including live reservations/failed reads.
    pub transfer_bytes_charged: u64,
}
/// No network IO during construction. Only ObjectSource::open contacts the server.
pub struct Source {
    config: Config,
    agent: ureq::Agent,
    budget: Arc<Mutex<Budget>>,
}
impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RemoteLfsSource(<private binding>)")
    }
}
struct Permit {
    budget: Arc<Mutex<Budget>>,
    remaining: u64,
    deadline: Instant,
}
impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut b) = self.budget.lock() {
            b.streams = b.streams.saturating_sub(1);
            b.bytes = b.bytes.saturating_sub(self.remaining);
        }
    }
}
pub struct Stream {
    reader: ureq::BodyReader<'static>,
    permit: Permit,
    eof: bool,
}
impl Source {
    pub fn place(&self) -> &Digest {
        &self.config.place
    }
    pub fn stats(&self) -> Result<Stats, SourceError> {
        let b = self.budget.lock().map_err(|_| SourceError::Unknown)?;
        Ok(Stats {
            requests_charged: b.requests,
            active_streams: b.streams,
            transfer_bytes_charged: b.bytes,
        })
    }
    fn request_budget(&self) -> Result<Duration, SourceError> {
        let mut b = self.budget.lock().map_err(|_| SourceError::Unknown)?;
        let left = b
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or(SourceError::Budget)?;
        if b.requests >= self.config.limits.max_requests {
            return Err(SourceError::Budget);
        }
        b.requests += 1;
        Ok(left.min(self.config.limits.request_timeout))
    }
    fn permit(&self, bytes: u64) -> Result<Permit, SourceError> {
        let mut b = self.budget.lock().map_err(|_| SourceError::Unknown)?;
        if b.streams >= self.config.limits.max_streams {
            return Err(SourceError::Busy);
        }
        if bytes > self.config.limits.max_total_bytes.saturating_sub(b.bytes) {
            return Err(SourceError::Budget);
        }
        b.streams += 1;
        b.bytes += bytes;
        Ok(Permit {
            budget: self.budget.clone(),
            remaining: bytes,
            deadline: b.deadline,
        })
    }
    fn batch(&self, object: &Object) -> Result<response::Action, SourceError> {
        let started = Instant::now();
        let timeout = self.request_budget()?;
        let body = serde_json::to_vec(
            &serde_json::json!({"operation":"download","transfers":["basic"],"hash_algo":"sha256",
            "objects":[{"oid":object.sha256.as_str(),"size":object.bytes}]}),
        )
        .map_err(|_| SourceError::Invalid)?;
        let mut req = self
            .agent
            .post(self.config.batch.to_string())
            .header("Accept", "application/vnd.git-lfs+json")
            .header("Content-Type", "application/vnd.git-lfs+json");
        if let Some(auth) = &self.config.batch_auth {
            req = req.header("Authorization", auth.0.clone());
        }
        let response = req
            .config()
            .timeout_global(Some(timeout))
            .build()
            .send(body.as_slice())
            .map_err(network_error)?;
        status(response.status().as_u16())?;
        if response
            .headers()
            .get("Content-Type")
            .and_then(|h| h.to_str().ok())
            .is_none_or(|h| {
                h.split(';').next().is_none_or(|m| {
                    !m.trim()
                        .eq_ignore_ascii_case("application/vnd.git-lfs+json")
                })
            })
        {
            return Err(SourceError::Unsupported);
        }
        representation(response.headers())?;
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(self.config.limits.metadata_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| SourceError::Unavailable)?;
        if bytes.len() > self.config.limits.metadata_bytes {
            return Err(SourceError::Budget);
        }
        response::parse(&self.config, object, &bytes, started)
    }
}
impl ObjectSource for Source {
    type Reader = Stream;
    fn max_object_bytes(&self) -> u64 {
        self.config.limits.max_object_bytes
    }
    fn open(&mut self, object: &Object) -> Result<Stream, SourceError> {
        if object.bytes > self.config.limits.max_object_bytes {
            return Err(SourceError::Budget);
        }
        let remaining = object.bytes.checked_add(1).ok_or(SourceError::Budget)?;
        let permit = self.permit(remaining)?;
        let action = self.batch(object)?;
        if Instant::now() >= action.deadline {
            return Err(SourceError::Stale);
        }
        let timeout = self.request_budget()?;
        if timeout.is_zero() {
            return Err(SourceError::Stale);
        }
        let mut req = self
            .agent
            .get(action.url.to_string())
            .header("Accept-Encoding", "identity");
        for (name, value) in action.headers {
            req = req.header(name, value);
        }
        let response = req
            .config()
            .timeout_global(Some(timeout))
            .build()
            .call()
            .map_err(network_error)?;
        status(response.status().as_u16())?;
        representation(response.headers())?;
        if let Some(length) = response.headers().get("Content-Length") {
            let length = length
                .to_str()
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or(SourceError::Invalid)?;
            if length != object.bytes {
                return Err(SourceError::Integrity);
            }
        }
        Ok(Stream {
            reader: response.into_body().into_reader(),
            permit,
            eof: false,
        })
    }
}
fn status(code: u16) -> Result<(), SourceError> {
    match code {
        200 => Ok(()),
        401 | 403 => Err(SourceError::Denied),
        404 | 410 | 500..=599 => Err(SourceError::Unavailable),
        300..=399 => Err(SourceError::Unsupported),
        _ => Err(SourceError::Unknown),
    }
}
fn representation(headers: &ureq::http::HeaderMap) -> Result<(), SourceError> {
    for value in headers.get_all("Content-Encoding") {
        if !value
            .to_str()
            .is_ok_and(|v| v.eq_ignore_ascii_case("identity"))
        {
            return Err(SourceError::Unsupported);
        }
    }
    if let Some(content_type) = headers.get("Content-Type") {
        let content_type = content_type.to_str().map_err(|_| SourceError::Invalid)?;
        for parameter in content_type.split(';').skip(1) {
            if let Some((name, value)) = parameter.split_once('=')
                && name.trim().eq_ignore_ascii_case("charset")
                && !value.trim().trim_matches('"').eq_ignore_ascii_case("utf-8")
            {
                return Err(SourceError::Unsupported);
            }
        }
    }
    Ok(())
}
fn network_error(error: ureq::Error) -> SourceError {
    match error {
        ureq::Error::TooManyRedirects | ureq::Error::RedirectFailed => SourceError::Unsupported,
        ureq::Error::Timeout(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Io(_) => SourceError::Unavailable,
        _ => SourceError::Unknown,
    }
}
fn read_error(kind: io::ErrorKind) -> io::Error {
    io::Error::new(kind, "remote LFS stream refused or unavailable")
}
impl Read for Stream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.eof {
            return Ok(0);
        }
        if self.permit.remaining == 0 {
            return Err(read_error(io::ErrorKind::InvalidData));
        }
        if Instant::now() >= self.permit.deadline {
            return Err(read_error(io::ErrorKind::TimedOut));
        }
        let reserved = (out.len().min(65_536) as u64).min(self.permit.remaining) as usize;
        let result = self.reader.read(&mut out[..reserved]);
        let consumed = result.as_ref().copied().unwrap_or(reserved);
        self.permit.remaining -= consumed as u64;
        let n = result.map_err(|e| read_error(e.kind()))?;
        self.eof = n == 0;
        Ok(n)
    }
}
