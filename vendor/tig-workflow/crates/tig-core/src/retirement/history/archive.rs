//! Exact bounded metadata staging/restore. Never refreshes time or admits authority.
use super::*;
use crate::tree::{NodeStore, TreeError};
use serde::de::DeserializeOwned;

const MAX_BYTES: u64 = 65_536;
const SCHEMA: &str = "tig.inactivity-archive.design-1";
const WITNESS: &str = "tig.inactivity-checkpoint.design-1";
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Budget,
    Encoding,
    Invalid,
    Integrity,
    Missing,
    Checkpoint,
    Metadata(TreeError),
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    epoch: Digest,
    interpretation: Digest,
    checkpoint: Checkpoint,
    previous: Option<Checkpoint>,
    snapshot: Digest,
    at: ClockPoint,
    tracked: BTreeMap<LogicalPath, Tracked>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Witness {
    schema: String,
    checkpoint: Checkpoint,
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::Encoding)?)
        .map_err(|_| Error::Encoding)
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let value = identity::parse_json(bytes).map_err(|_| Error::Encoding)?;
    if identity::canonical_json(&value).map_err(|_| Error::Encoding)? != bytes {
        return Err(Error::Invalid);
    }
    serde_json::from_value(value).map_err(|_| Error::Invalid)
}
pub(super) fn encode_checkpoint(checkpoint: &Checkpoint) -> Result<Vec<u8>> {
    encode(&Witness {
        schema: WITNESS.into(),
        checkpoint: checkpoint.clone(),
    })
}
pub(super) fn decode_checkpoint(bytes: &[u8]) -> Result<Checkpoint> {
    let witness: Witness = decode(bytes)?;
    if witness.schema != WITNESS {
        return Err(Error::Invalid);
    }
    Ok(witness.checkpoint)
}
/// Returns a candidate reference only. Provider finish/protection is separate.
pub fn stage(history: &History, nodes: &mut impl NodeStore) -> Result<Object> {
    let record = Record {
        schema: SCHEMA.into(),
        epoch: history.epoch.clone(),
        interpretation: history.interpretation.clone(),
        checkpoint: history.checkpoint.clone(),
        previous: history.previous.clone(),
        snapshot: history.snapshot.clone(),
        at: history.at.clone(),
        tracked: history.tracked.clone(),
    };
    let bytes = encode(&record)?;
    let expected = Object::from_bytes(&bytes);
    let acknowledged = nodes.put(&bytes).map_err(Error::Metadata)?;
    if acknowledged != expected {
        return Err(Error::Integrity);
    }
    let readback = nodes
        .get(&expected)
        .map_err(Error::Metadata)?
        .ok_or(Error::Missing)?;
    if readback != bytes {
        return Err(Error::Integrity);
    }
    Ok(expected)
}
/// Exact expected witness is caller-admitted; neither parsing nor restoration
/// proves it is globally current. This method performs no writes or time refresh.
pub fn restore(
    reference: &Object,
    nodes: &impl NodeStore,
    expected: &Checkpoint,
) -> Result<History> {
    if reference.bytes == 0 || reference.bytes > MAX_BYTES {
        return Err(Error::Budget);
    }
    let bytes = nodes
        .get(reference)
        .map_err(Error::Metadata)?
        .ok_or(Error::Missing)?;
    if bytes.len() != reference.bytes as usize || Object::from_bytes(&bytes) != *reference {
        return Err(Error::Integrity);
    }
    let record: Record = decode(&bytes)?;
    if record.schema != SCHEMA {
        return Err(Error::Invalid);
    }
    if &record.checkpoint != expected {
        return Err(Error::Checkpoint);
    }
    if record.tracked.len() > MAX_TRACKED {
        return Err(Error::Budget);
    }
    match &record.previous {
        None if record.checkpoint.sequence != 0 => return Err(Error::Invalid),
        Some(p)
            if p.epoch != record.epoch
                || p.sequence.checked_add(1) != Some(record.checkpoint.sequence) =>
        {
            return Err(Error::Invalid);
        }
        _ => (),
    }
    crate::check_sizes(record.tracked.values().map(|t| &t.binding.object))
        .map_err(|_| Error::Invalid)?;
    for tracked in record.tracked.values() {
        if let Activity::Inactive(i) = &tracked.binding.activity {
            if i.clock != record.at.era || i.started > i.through || i.through != record.at.tick {
                return Err(Error::Invalid);
            }
            if record.previous.is_none() && i.started != record.at.tick {
                return Err(Error::Invalid);
            }
        }
    }
    let observations = record
        .tracked
        .iter()
        .map(|(p, t)| {
            (
                p.clone(),
                Observation {
                    object: t.binding.object.clone(),
                    classification: t.classification.clone(),
                    status: match t.binding.activity {
                        Activity::Unknown => Status::Unknown,
                        Activity::Active => Status::Active,
                        Activity::Inactive(_) => Status::Inactive,
                    },
                },
            )
        })
        .collect();
    let header = Header {
        epoch: &record.epoch,
        interpretation: &record.interpretation,
        sequence: record.checkpoint.sequence,
        previous: record.previous.as_ref(),
        snapshot: &record.snapshot,
        at: &record.at,
    };
    let frame = header.frame(&observations).map_err(|_| Error::Encoding)?;
    if record.previous.is_none() {
        for (path, tracked) in &record.tracked {
            if let Activity::Inactive(i) = &tracked.binding.activity
                && i.identity
                    != digest(&("tig.inactive-interval.design-1", &frame, path))
                        .map_err(|_| Error::Encoding)?
            {
                return Err(Error::Invalid);
            }
        }
    }
    let checkpoint = header
        .checkpoint(&frame, &record.tracked)
        .map_err(|_| Error::Encoding)?;
    if checkpoint != record.checkpoint {
        return Err(Error::Integrity);
    }
    Ok(History {
        epoch: record.epoch,
        interpretation: record.interpretation,
        checkpoint,
        previous: record.previous,
        snapshot: record.snapshot,
        at: record.at,
        tracked: record.tracked,
    })
}
