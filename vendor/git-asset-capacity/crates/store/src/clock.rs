//! Explicit OS time admission backed by the existing bounded capacity journal.
//! No discovery, fallback, live enrollment or external UTC correctness claim.
use crate::{
    Key,
    final_retirement::provider,
    ledger::{CapacityLedger, Checkpoint, LedgerError},
};
use git_asset_core::{Digest, identity};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub enum Error {
    InvalidPolicy,
    WrongEnrollment,
    Time,
    Expired,
    Busy,
    Ledger(LedgerError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<LedgerError> for Error {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
fn digest(value: &impl Serialize) -> Result<Digest> {
    let value = serde_json::to_value(value).map_err(|_| Error::InvalidPolicy)?;
    Ok(Digest::blob(
        &identity::canonical_json(&value).map_err(|_| Error::InvalidPolicy)?,
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub authority: Digest,
    pub epoch: Digest,
    /// Independently admitted native OS time/trust boundary, not NTP discovery.
    pub boundary: Digest,
    pub max_sample_millis: u64,
    pub max_drift_millis: u64,
    pub max_session_millis: u64,
}
impl Policy {
    pub fn id(&self) -> Result<Digest> {
        if self.max_sample_millis == 0
            || self.max_sample_millis > 60_000
            || self.max_drift_millis == 0
            || self.max_drift_millis > 60_000
            || self.max_session_millis < self.max_sample_millis
            || self.max_session_millis > 604_800_000
        {
            return Err(Error::InvalidPolicy);
        }
        digest(&serde_json::json!({"schema":"tig.local-clock-policy.design-1","value":self}))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub journal: Digest,
    pub policy: Policy,
}
impl Enrollment {
    pub fn id(&self) -> Result<Digest> {
        self.policy.id()?;
        digest(&serde_json::json!({"schema":"tig.local-clock-enrollment.design-1","value":self}))
    }
}
#[derive(Debug, Serialize)]
pub struct Status {
    pub schema: &'static str,
    pub enrollment: Digest,
    pub checkpoint: Checkpoint,
    pub accepted_lower_bound: Option<u64>,
    pub current_time_authority: bool,
}
struct State {
    ledger: CapacityLedger,
    anchor: Option<(u64, Instant)>,
    last: Option<(u64, Instant)>,
    failed: bool,
}
pub struct LocalClock {
    enrollment: Enrollment,
    state: RefCell<State>,
}
#[derive(Clone, Copy)]
struct Sample {
    wall: u64,
    before: Instant,
    after: Instant,
}
fn native_sample() -> Result<Sample> {
    let before = Instant::now();
    let wall = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Time)?
            .as_millis(),
    )
    .map_err(|_| Error::Time)?;
    Ok(Sample {
        wall,
        before,
        after: Instant::now(),
    })
}
fn elapsed(later: Instant, earlier: Instant) -> Result<u64> {
    u64::try_from(
        later
            .checked_duration_since(earlier)
            .ok_or(Error::Time)?
            .as_millis(),
    )
    .map_err(|_| Error::Time)
}
impl LocalClock {
    /// The caller opens the existing ledger with its independently trusted minimum.
    /// Construction itself samples no clock and appends no command.
    pub fn new(ledger: CapacityLedger, expected: Enrollment) -> Result<Self> {
        expected.id()?;
        if ledger.clock_enrollment()? != expected {
            return Err(Error::WrongEnrollment);
        }
        Ok(Self {
            enrollment: expected,
            state: RefCell::new(State {
                ledger,
                anchor: None,
                last: None,
                failed: false,
            }),
        })
    }
    pub fn enrollment(&self) -> &Enrollment {
        &self.enrollment
    }
    /// Historical floor/checkpoint only. Never current-time or deletion authority.
    pub fn status(&self) -> Result<Status> {
        let mut state = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
        let accepted_lower_bound = state.ledger.clock_high_water()?;
        Ok(Status {
            schema: "tig.local-clock-status.design-1",
            enrollment: self.enrollment.id()?,
            checkpoint: state.ledger.checkpoint(),
            accepted_lower_bound,
            current_time_authority: false,
        })
    }
    pub fn sample(&self, policy: &Digest) -> Result<provider::TimeBounds> {
        self.sample_with(policy, native_sample)
    }
    fn sample_with(
        &self,
        policy: &Digest,
        mut sample: impl FnMut() -> Result<Sample>,
    ) -> Result<provider::TimeBounds> {
        if policy != &self.enrollment.id()? {
            return Err(Error::WrongEnrollment);
        }
        let mut state = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
        if state.failed {
            return Err(Error::Time);
        }
        let result = self.observe(&mut state, &mut sample);
        if matches!(result, Err(Error::Time | Error::Expired)) {
            state.failed = true;
        }
        result
    }
    fn observe(
        &self,
        state: &mut State,
        sample: &mut impl FnMut() -> Result<Sample>,
    ) -> Result<provider::TimeBounds> {
        let policy = &self.enrollment.policy;
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce).map_err(|_| Error::Time)?;
        let operation = Key::parse(&format!("time-{}", Digest::blob(&nonce).as_str()))
            .map_err(|_| Error::Time)?;
        let first = sample()?;
        self.check(state, first)?;
        let latest = first
            .wall
            .checked_add(policy.max_sample_millis)
            .and_then(|n| n.checked_add(policy.max_drift_millis))
            .ok_or(Error::Time)?;
        // Reject timestamps outside the existing canonical-number contract
        // before trying to append a malformed journal command.
        identity::canonical_json(&serde_json::json!([first.wall, latest]))
            .map_err(|_| Error::Time)?;
        let receipt = state
            .ledger
            .observe_clock(operation, first.wall, state.anchor.is_none())?;
        if receipt.was_replayed() {
            return Err(Error::Time);
        }
        let last = sample()?;
        self.check(state, last)?;
        elapsed(last.before, first.after)?;
        let age = elapsed(last.after, first.before)?;
        let advance = last.wall.checked_sub(first.wall).ok_or(Error::Time)?;
        if age > policy.max_sample_millis || advance.abs_diff(age) > policy.max_drift_millis {
            return Err(Error::Time);
        }
        if last.wall > latest {
            return Err(Error::Time);
        }
        state.anchor.get_or_insert((first.wall, first.before));
        state.last = Some((last.wall, last.after));
        Ok(provider::TimeBounds {
            earliest: first.wall,
            latest,
        })
    }
    fn check(&self, state: &State, sample: Sample) -> Result<()> {
        let policy = &self.enrollment.policy;
        if sample.wall == 0 || elapsed(sample.after, sample.before)? > policy.max_sample_millis {
            return Err(Error::Time);
        }
        if let Some((wall, mark)) = state.last {
            elapsed(sample.before, mark)?;
            if sample.wall < wall {
                return Err(Error::Time);
            }
        }
        if let Some((wall, mark)) = state.anchor {
            let age = elapsed(sample.after, mark)?;
            if age > policy.max_session_millis {
                return Err(Error::Expired);
            }
            let advance = sample.wall.checked_sub(wall).ok_or(Error::Time)?;
            if advance.abs_diff(age) > policy.max_drift_millis {
                return Err(Error::Time);
            }
        }
        Ok(())
    }
}
impl provider::Clock for LocalClock {
    fn now(&self, policy: &Digest) -> provider::Result<u64> {
        Ok(self
            .sample(policy)
            .map_err(|_| provider::Error::Clock)?
            .earliest)
    }
    fn bounds(&self, policy: &Digest) -> provider::Result<provider::TimeBounds> {
        self.sample(policy).map_err(|_| provider::Error::Clock)
    }
}

#[cfg(test)]
mod tests;
