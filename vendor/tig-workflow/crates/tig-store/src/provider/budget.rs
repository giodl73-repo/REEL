//! Shared operation-scoped source budgets. Counts port work, not hidden wire IO.
use super::{ExactSource, Located, Lookup, Opened, ReadCapabilities};
use crate::acquire::SourceError;
use std::{
    io::{self, Read},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub calls: u64,
    pub read_calls: u64,
    /// Aggregate stream charge; allow an excess-detection byte when required.
    pub source_bytes: u64,
    pub live_streams: u64,
    pub read_chunk: usize,
    pub elapsed: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub lookups: u64,
    pub opens: u64,
    pub read_calls: u64,
    /// Returned bytes plus in-flight reservations and failed-read allowances.
    pub source_bytes_charged: u64,
    pub source_bytes_observed: u64,
    pub live_streams: u64,
    pub peak_streams: u64,
    pub largest_read_request: usize,
}

struct State {
    limits: Limits,
    deadline: Instant,
    cancelled: bool,
    stats: Stats,
}
impl State {
    fn ready(&self) -> Result<(), SourceError> {
        if self.cancelled || Instant::now() >= self.deadline {
            Err(SourceError::Budget)
        } else {
            Ok(())
        }
    }
}

/// Clone shares limits, charges, cancellation and deadline; never resets retries.
/// No lock spans provider IO. This budget is not a cache-wide admission lock.
#[derive(Clone)]
pub struct Budget(Arc<Mutex<State>>);
impl Budget {
    pub fn new(limits: Limits) -> Result<Self, SourceError> {
        if limits.live_streams == 0
            || !(1..=65_536).contains(&limits.read_chunk)
            || limits.elapsed.is_zero()
            || limits.elapsed > Duration::from_secs(86_400)
        {
            return Err(SourceError::Invalid);
        }
        let deadline = Instant::now()
            .checked_add(limits.elapsed)
            .ok_or(SourceError::Invalid)?;
        Ok(Self(Arc::new(Mutex::new(State {
            limits,
            deadline,
            cancelled: false,
            stats: Stats::default(),
        }))))
    }
    pub fn stats(&self) -> Result<Stats, SourceError> {
        Ok(self.0.lock().map_err(|_| SourceError::Unknown)?.stats)
    }
    /// Cooperative boundary cancellation; does not interrupt synchronous IO or
    /// prove that an in-flight provider operation had no effect.
    pub fn cancel(&self) -> Result<(), SourceError> {
        self.0.lock().map_err(|_| SourceError::Unknown)?.cancelled = true;
        Ok(())
    }
    fn call(&self, open: bool) -> Result<(), SourceError> {
        let mut s = self.0.lock().map_err(|_| SourceError::Unknown)?;
        s.ready()?;
        let total = s
            .stats
            .lookups
            .checked_add(s.stats.opens)
            .ok_or(SourceError::Budget)?;
        if total >= s.limits.calls {
            return Err(SourceError::Budget);
        }
        if open {
            if s.stats.live_streams >= s.limits.live_streams {
                return Err(SourceError::Busy);
            }
            s.stats.opens += 1;
            s.stats.live_streams += 1;
            s.stats.peak_streams = s.stats.peak_streams.max(s.stats.live_streams);
        } else {
            s.stats.lookups += 1;
        }
        Ok(())
    }
    fn ready(&self) -> Result<(), SourceError> {
        self.0.lock().map_err(|_| SourceError::Unknown)?.ready()
    }
}

struct Permit(Budget);
impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut s) = self.0.0.lock() {
            s.stats.live_streams -= 1;
        }
    }
}

pub struct Stream<R: Read> {
    reader: R,
    permit: Permit,
    eof: bool,
}
impl<R: Read> Read for Stream<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if into.is_empty() || self.eof {
            return Ok(0);
        }
        let budget = &self.permit.0;
        let allowance = {
            let mut s = budget
                .0
                .lock()
                .map_err(|_| io::Error::other(SourceError::Unknown))?;
            s.ready().map_err(io::Error::other)?;
            let remaining = s.limits.source_bytes - s.stats.source_bytes_charged;
            if s.stats.read_calls >= s.limits.read_calls || remaining == 0 {
                return Err(io::Error::other(SourceError::Budget));
            }
            let n = into
                .len()
                .min(s.limits.read_chunk)
                .min(remaining.min(65_536) as usize);
            s.stats.read_calls += 1;
            s.stats.source_bytes_charged += n as u64;
            s.stats.largest_read_request = s.stats.largest_read_request.max(n);
            n
        };
        // On errors, including Interrupted, conservatively retain the allowance.
        // Provider-internal work on a failed read cannot be recovered from Read.
        let result = self.reader.read(&mut into[..allowance]);
        if let Ok(n) = result {
            if n > allowance {
                return Err(io::Error::other(SourceError::Integrity));
            }
            let mut s = budget
                .0
                .lock()
                .map_err(|_| io::Error::other(SourceError::Unknown))?;
            s.stats.source_bytes_charged -= (allowance - n) as u64;
            s.stats.source_bytes_observed += n as u64;
            s.ready().map_err(io::Error::other)?;
            self.eof = n == 0;
        }
        result
    }
}

/// Captures this adapter's read capability descriptor and refuses changes.
/// Construction is IO-free; caller-admitted binding identity and qualification
/// remain separate. Same-descriptor endpoint changes cannot be detected here.
pub struct Source<'a, S: ExactSource> {
    source: &'a mut S,
    profile: ReadCapabilities,
    budget: Budget,
}
impl<'a, S: ExactSource> Source<'a, S> {
    pub fn new(source: &'a mut S, budget: Budget) -> Self {
        Self {
            profile: source.describe(),
            source,
            budget,
        }
    }
    fn check(&self) -> Result<(), SourceError> {
        self.budget.ready()?;
        if self.source.describe() != self.profile {
            Err(SourceError::Stale)
        } else {
            Ok(())
        }
    }
}
impl<S: ExactSource> ExactSource for Source<'_, S> {
    type Reader = Stream<S::Reader>;
    fn describe(&self) -> ReadCapabilities {
        self.profile.clone()
    }
    fn lookup(&mut self, object: &tig_core::Object) -> Result<Lookup, SourceError> {
        self.check()?;
        self.budget.call(false)?;
        let result = self.source.lookup(object);
        self.check()?;
        result
    }
    fn open_version(&mut self, location: &Located) -> Result<Opened<Self::Reader>, SourceError> {
        self.check()?;
        self.budget.call(true)?;
        let permit = Permit(self.budget.clone());
        let opened = self.source.open_version(location)?;
        self.check()?;
        Ok(Opened {
            location: opened.location,
            reader: Stream {
                reader: opened.reader,
                permit,
                eof: false,
            },
        })
    }
}
