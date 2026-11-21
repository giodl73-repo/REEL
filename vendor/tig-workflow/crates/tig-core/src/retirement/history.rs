//! Ordered, bounded interval projection. Source/clock admission remains external.
use super::{Activity, Binding, Catalogue, ClockPoint, InactiveInterval};
use crate::{Digest, LogicalPath, Object, identity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub mod archive;

pub const MAX_TRACKED: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Status {
    Unknown,
    Active,
    Inactive,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Observation {
    pub object: Object,
    pub status: Status,
    /// Admitted identity of the binding's eligibility class, not a free label.
    pub classification: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    epoch: Digest,
    sequence: u64,
    digest: Digest,
}
impl Checkpoint {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn digest(&self) -> &Digest {
        &self.digest
    }
    pub fn to_json(&self) -> archive::Result<Vec<u8>> {
        archive::encode_checkpoint(self)
    }
    /// Parsing does not authenticate or admit this witness as current.
    pub fn from_json(bytes: &[u8]) -> archive::Result<Self> {
        archive::decode_checkpoint(bytes)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Budget,
    ConflictingSize,
    StaleCheckpoint,
    Sequence,
    ClockChanged,
    Encoding,
    Overflow,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tracked {
    classification: Digest,
    binding: Binding,
}
#[derive(Clone, Debug)]
pub struct History {
    epoch: Digest,
    interpretation: Digest,
    checkpoint: Checkpoint,
    previous: Option<Checkpoint>,
    at: ClockPoint,
    snapshot: Digest,
    tracked: BTreeMap<LogicalPath, Tracked>,
}
type Result<T> = std::result::Result<T, Error>;
fn digest(value: &impl Serialize) -> Result<Digest> {
    let value = serde_json::to_value(value).map_err(|_| Error::Encoding)?;
    Ok(Digest::blob(
        &identity::canonical_json(&value).map_err(|_| Error::Encoding)?,
    ))
}
struct Header<'a> {
    epoch: &'a Digest,
    interpretation: &'a Digest,
    sequence: u64,
    previous: Option<&'a Checkpoint>,
    snapshot: &'a Digest,
    at: &'a ClockPoint,
}
impl Header<'_> {
    fn frame(&self, observations: &BTreeMap<LogicalPath, Observation>) -> Result<Digest> {
        digest(&(
            "tig.inactivity-frame.design-1",
            self.epoch,
            self.interpretation,
            self.sequence,
            self.previous,
            self.snapshot,
            self.at,
            observations,
        ))
    }
    fn checkpoint(
        &self,
        frame: &Digest,
        tracked: &BTreeMap<LogicalPath, Tracked>,
    ) -> Result<Checkpoint> {
        Ok(Checkpoint {
            epoch: self.epoch.clone(),
            sequence: self.sequence,
            digest: digest(&(
                "tig.inactivity-history.design-1",
                self.epoch,
                self.interpretation,
                self.sequence,
                self.previous,
                self.snapshot,
                self.at,
                frame,
                tracked,
            ))?,
        })
    }
}
impl History {
    pub fn new(
        epoch: Digest,
        interpretation: Digest,
        snapshot: Digest,
        at: ClockPoint,
        observations: BTreeMap<LogicalPath, Observation>,
    ) -> Result<Self> {
        Self::project(None, epoch, interpretation, 0, snapshot, at, observations)
    }
    fn project(
        previous: Option<&Self>,
        epoch: Digest,
        interpretation: Digest,
        sequence: u64,
        snapshot: Digest,
        at: ClockPoint,
        observations: BTreeMap<LogicalPath, Observation>,
    ) -> Result<Self> {
        if observations.len() > MAX_TRACKED {
            return Err(Error::Budget);
        }
        crate::check_sizes(observations.values().map(|o| &o.object))
            .map_err(|_| Error::ConflictingSize)?;
        let header = Header {
            epoch: &epoch,
            interpretation: &interpretation,
            sequence,
            previous: previous.map(|h| &h.checkpoint),
            snapshot: &snapshot,
            at: &at,
        };
        let frame = header.frame(&observations)?;
        let mut tracked = BTreeMap::new();
        for (path, observation) in observations {
            let old = previous.and_then(|h| h.tracked.get(&path)).filter(|old| {
                old.binding.object == observation.object
                    && old.classification == observation.classification
            });
            let activity = match observation.status {
                Status::Unknown => Activity::Unknown,
                Status::Active => Activity::Active,
                Status::Inactive => {
                    let continued = old.and_then(|old| match &old.binding.activity {
                        Activity::Inactive(i) => Some(i),
                        _ => None,
                    });
                    let interval = if let Some(interval) = continued {
                        InactiveInterval {
                            through: at.tick,
                            ..interval.clone()
                        }
                    } else {
                        InactiveInterval {
                            clock: at.era.clone(),
                            identity: digest(&("tig.inactive-interval.design-1", &frame, &path))?,
                            started: at.tick,
                            through: at.tick,
                        }
                    };
                    Activity::Inactive(interval)
                }
            };
            tracked.insert(
                path,
                Tracked {
                    classification: observation.classification,
                    binding: Binding {
                        object: observation.object,
                        activity,
                    },
                },
            );
        }
        let checkpoint = header.checkpoint(&frame, &tracked)?;
        Ok(Self {
            epoch,
            interpretation,
            checkpoint,
            previous: previous.map(|h| h.checkpoint.clone()),
            snapshot,
            at,
            tracked,
        })
    }
    /// Requires an admitted contiguous event stream, not locally numbered samples.
    pub fn advance(
        &self,
        expected: &Checkpoint,
        sequence: u64,
        snapshot: Digest,
        at: ClockPoint,
        observations: BTreeMap<LogicalPath, Observation>,
    ) -> Result<Self> {
        if expected != &self.checkpoint {
            return Err(Error::StaleCheckpoint);
        }
        if sequence
            != self
                .checkpoint
                .sequence
                .checked_add(1)
                .ok_or(Error::Overflow)?
        {
            return Err(Error::Sequence);
        }
        if at.era != self.at.era || at.tick < self.at.tick {
            return Err(Error::ClockChanged);
        }
        Self::project(
            Some(self),
            self.epoch.clone(),
            self.interpretation.clone(),
            sequence,
            snapshot,
            at,
            observations,
        )
    }
    pub fn checkpoint(&self) -> Checkpoint {
        self.checkpoint.clone()
    }
    pub fn previous_checkpoint(&self) -> Option<Checkpoint> {
        self.previous.clone()
    }
    /// Identity comparison only; not source/clock authentication.
    pub fn same_stream(&self, other: &Self) -> bool {
        self.epoch == other.epoch
            && self.interpretation == other.interpretation
            && self.at.era == other.at.era
    }
    pub fn at(&self) -> ClockPoint {
        self.at.clone()
    }
    pub fn catalogue(&self) -> Catalogue {
        Catalogue {
            snapshot: self.snapshot.clone(),
            bindings: self
                .tracked
                .iter()
                .map(|(p, t)| (p.clone(), t.binding.clone()))
                .collect(),
        }
    }
}
