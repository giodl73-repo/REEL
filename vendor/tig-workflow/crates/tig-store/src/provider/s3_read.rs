//! Exact versioned S3 reads over explicit method-specific URLs. No discovery/signing.
use super::{ExactSource, Located, Lookup, Opened, ReadCapabilities, ReadProtection, Version};
use crate::acquire::SourceError;
use std::{
    io::{self, Read},
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tig_core::{Digest, Object};
use ureq::{
    http::Uri,
    tls::{Certificate, RootCerts, TlsConfig},
};
mod binding;
pub use binding::Config;

#[derive(Clone, Debug)]
pub struct Limits {
    pub max_object_bytes: u64,
    pub max_requests: u32,
    pub max_streams: usize,
    pub max_total_bytes: u64,
    pub request_timeout: Duration,
    pub session_timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_object_bytes: 64 << 30,
            max_requests: 128,
            max_streams: 4,
            max_total_bytes: 256 << 30,
            request_timeout: Duration::from_secs(30),
            session_timeout: Duration::from_secs(300),
        }
    }
}
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
    /// Conservative payload charge, including bytes consumed by failed transfers.
    pub transfer_bytes_charged: u64,
}
pub struct Source {
    config: Config,
    agent: ureq::Agent,
    budget: Arc<Mutex<Budget>>,
}
impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("S3ReadSource(<private binding>)")
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
            .filter(|x| !x.is_zero())
            .ok_or(SourceError::Budget)?;
        if b.requests >= self.config.limits.max_requests {
            return Err(SourceError::Budget);
        }
        b.requests += 1;
        Ok(left.min(self.config.limits.request_timeout))
    }
    fn permit(&self) -> Result<Permit, SourceError> {
        let mut b = self.budget.lock().map_err(|_| SourceError::Unknown)?;
        if b.streams >= self.config.limits.max_streams {
            return Err(SourceError::Busy);
        }
        if self.config.object.bytes > self.config.limits.max_total_bytes.saturating_sub(b.bytes) {
            return Err(SourceError::Budget);
        }
        b.streams += 1;
        b.bytes += self.config.object.bytes;
        Ok(Permit {
            budget: self.budget.clone(),
            remaining: self.config.object.bytes,
            deadline: b.deadline,
        })
    }
    fn location(&self) -> Located {
        Located {
            place: self.config.place.clone(),
            object: self.config.object.clone(),
            version: self.config.version.clone(),
        }
    }
    fn headers(&self, response: &ureq::http::Response<ureq::Body>) -> Result<(), SourceError> {
        let h = response.headers();
        let version = h
            .get("x-amz-version-id")
            .and_then(|v| v.to_str().ok())
            .ok_or(SourceError::Stale)?;
        if version.as_bytes() != self.config.version.as_bytes() {
            return Err(SourceError::Stale);
        }
        if h.get_all("x-amz-version-id").iter().count() != 1
            || h.get_all("content-length").iter().count() != 1
            || h.contains_key("content-range")
            || h.contains_key("transfer-encoding")
            || h.contains_key("x-amz-delete-marker")
            || h.get("content-encoding")
                .is_some_and(|v| v.as_bytes() != b"identity")
        {
            return Err(SourceError::Unsupported);
        }
        let length = h
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(SourceError::Invalid)?;
        if length != self.config.object.bytes {
            return Err(SourceError::Integrity);
        }
        Ok(())
    }
}
fn status(code: u16) -> Result<(), SourceError> {
    match code {
        200 => Ok(()),
        401 | 403 => Err(SourceError::Denied),
        404 | 405 | 410 => Err(SourceError::Unavailable),
        409 | 412 => Err(SourceError::Stale),
        429 => Err(SourceError::Busy),
        500..=599 => Err(SourceError::Unavailable),
        300..=399 => Err(SourceError::Denied),
        _ => Err(SourceError::Unsupported),
    }
}
fn network(e: ureq::Error) -> SourceError {
    match e {
        ureq::Error::Timeout(_) => SourceError::Budget,
        _ => SourceError::Unavailable,
    }
}
impl ExactSource for Source {
    type Reader = Stream;
    fn describe(&self) -> ReadCapabilities {
        ReadCapabilities {
            place: self.config.place.clone(),
            max_object_bytes: self.config.limits.max_object_bytes,
            protection: ReadProtection::ImmutableVersion,
        }
    }
    fn lookup(&mut self, object: &Object) -> Result<Lookup, SourceError> {
        if object != &self.config.object {
            return Err(SourceError::Invalid);
        }
        if object.bytes > self.config.limits.max_object_bytes {
            return Err(SourceError::Budget);
        }
        let timeout = self.request_budget()?;
        let response = self
            .agent
            .head(self.config.head.to_string())
            .header("Accept-Encoding", "identity")
            .config()
            .timeout_global(Some(timeout))
            .build()
            .call()
            .map_err(network)?;
        status(response.status().as_u16())?;
        self.headers(&response)?;
        Ok(Lookup::Present(self.location()))
    }
    fn open_version(&mut self, location: &Located) -> Result<Opened<Stream>, SourceError> {
        if location != &self.location() {
            return Err(SourceError::Stale);
        }
        if location.object.bytes > self.config.limits.max_object_bytes {
            return Err(SourceError::Budget);
        }
        let permit = self.permit()?;
        let timeout = self.request_budget()?;
        let response = self
            .agent
            .get(self.config.get.to_string())
            .header("Accept-Encoding", "identity")
            .config()
            .timeout_global(Some(timeout))
            .build()
            .call()
            .map_err(network)?;
        status(response.status().as_u16())?;
        self.headers(&response)?;
        Ok(Opened {
            location: self.location(),
            reader: Stream {
                reader: response.into_body().into_reader(),
                permit,
                eof: false,
            },
        })
    }
}
impl Read for Stream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.eof {
            return Ok(0);
        }
        if Instant::now() >= self.permit.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "S3 read budget expired",
            ));
        }
        if self.permit.remaining == 0 {
            let mut extra = [0];
            if self.reader.read(&mut extra)? != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "S3 excess bytes",
                ));
            }
            self.eof = true;
            return Ok(0);
        }
        let cap = out
            .len()
            .min(65_536)
            .min(self.permit.remaining.min(usize::MAX as u64) as usize);
        let n = self.reader.read(&mut out[..cap])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "S3 truncated version",
            ));
        }
        self.permit.remaining -= n as u64;
        Ok(n)
    }
}
