//! Small consumer port: IDs to byte identities, independent of Git/cache layout.
use crate::{graph::Reference, native::BoundInstance};
use serde_json::{Map, Value};
use std::sync::Arc;
use tig_core::{Digest, Object};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Missing,
    Planned,
    MissingStream,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

/// Producer consumers (such as REEL) depend on this port, not repository paths.
/// Implementations must supply one explicit immutable selection view.
pub trait AssetResolver {
    fn resolve_id(&self, id: &str) -> Result<Resolved, Error>;
}

/// Bound metadata identities only. No payload integrity/freshness/custody claim.
#[derive(Clone)]
pub struct Resolved {
    checkpoint: Digest,
    reference: Reference,
    definition: Reference,
    selected: Option<Object>,
    streams: Vec<StreamBinding>,
    authored: Arc<Map<String, Value>>,
    status: Arc<Map<String, Value>>,
    metadata: Arc<Map<String, Value>>,
}

#[derive(Clone, Debug)]
pub struct StreamBinding {
    pub slot: String,
    pub role: String,
    pub group: String,
    pub object: Object,
}
impl Resolved {
    /// The caller supplies the qualified source checkpoint; bind proves metadata
    /// pins, not the caller's source enrollment or production authorization.
    pub fn from_bound(checkpoint: Digest, bound: &BoundInstance) -> Self {
        let value = bound.instance().value();
        Self {
            checkpoint,
            reference: Reference {
                id: value.id.clone(),
                object: bound.instance().object().clone(),
            },
            definition: Reference {
                id: bound.definition().value().id.clone(),
                object: bound.definition().object().clone(),
            },
            selected: value.selected.as_ref().map(|p| p.object()),
            authored: Arc::new(value.authored.clone()),
            status: Arc::new(value.status.clone()),
            metadata: Arc::new(value.metadata.clone()),
            streams: value
                .streams
                .iter()
                .map(|s| StreamBinding {
                    slot: s.slot.clone(),
                    role: s.role.clone(),
                    group: s.group.clone(),
                    object: s.object.object(),
                })
                .collect(),
        }
    }
    pub fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub fn reference(&self) -> &Reference {
        &self.reference
    }
    pub fn definition(&self) -> &Reference {
        &self.definition
    }
    pub fn selected(&self) -> Result<&Object, Error> {
        self.selected.as_ref().ok_or(Error::Planned)
    }
    pub fn stream(&self, slot: &str) -> Result<&Object, Error> {
        self.streams
            .iter()
            .find(|s| s.slot == slot)
            .map(|s| &s.object)
            .ok_or(Error::MissingStream)
    }
    pub fn stream_bindings(&self) -> &[StreamBinding] {
        &self.streams
    }
    pub fn authored(&self) -> &Map<String, Value> {
        &self.authored
    }
    /// Project-owned values from this exact record revision. This does not
    /// interpret eligibility or grant production, publication or removal authority.
    pub fn status(&self) -> &Map<String, Value> {
        &self.status
    }
    /// Private project metadata from this exact record. Values and evidence pins
    /// are data, not authority; Debug and command output do not expose this map.
    pub fn metadata(&self) -> &Map<String, Value> {
        &self.metadata
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolved")
            .field("checkpoint", &self.checkpoint)
            .field("reference", &self.reference)
            .field("definition", &self.definition)
            .field("stream_count", &self.streams.len())
            .finish_non_exhaustive()
    }
}
