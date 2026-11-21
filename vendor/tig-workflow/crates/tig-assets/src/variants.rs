//! Adapted from Art119c6644 variants (MIT, copyright Gio Della-Libera2026).
//! Exact sparse native selection only; never freshness, approval or custody.
use crate::{graph::Reference, native::MAX_RECORD_BYTES, resolve::Resolved};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{
    Digest, Object,
    identity::{canonical_json, parse_json},
};

pub type Parameters = BTreeMap<String, String>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Pin,
    Json,
    Space,
    Index,
    Admission,
    Context,
    Missing,
    Stream,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
fn decode<T: serde::de::DeserializeOwned>(pin: &Object, raw: &[u8]) -> Result<T, Error> {
    if raw.len() > MAX_RECORD_BYTES
        || pin.bytes > MAX_RECORD_BYTES as u64
        || pin != &Object::from_bytes(raw)
    {
        return Err(Error::Pin);
    }
    serde_json::from_value(parse_json(raw).map_err(|_| Error::Json)?).map_err(|_| Error::Json)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceWire {
    schema: String,
    axes: BTreeMap<String, Vec<String>>,
}
pub struct Space {
    object: Object,
    axes: BTreeMap<String, BTreeSet<String>>,
}
impl Space {
    pub fn parse(pin: &Object, raw: &[u8]) -> Result<Self, Error> {
        let wire: SpaceWire = decode(pin, raw)?;
        if wire.schema != "tig.variant-space.experimental-1" || wire.axes.len() > 16 {
            return Err(Error::Space);
        }
        let mut axes = BTreeMap::new();
        for (axis, values) in wire.axes {
            let set: BTreeSet<_> = values.iter().cloned().collect();
            if !token(&axis)
                || values.is_empty()
                || values.len() > 64
                || set.len() != values.len()
                || !values.iter().all(|v| token(v))
            {
                return Err(Error::Space);
            }
            axes.insert(axis, set);
        }
        Ok(Self {
            object: pin.clone(),
            axes,
        })
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn admit(&self, parameters: &Parameters) -> Result<Context, Error> {
        self.context(parameters)?;
        let bytes=canonical_json(&json!({"schema":"tig.variant-context.experimental-1","space":self.object,"parameters":parameters})).map_err(|_|Error::Json)?;
        Ok(Context {
            space: self.object.clone(),
            parameters: parameters.clone(),
            object: Object::from_bytes(&bytes),
            bytes,
        })
    }
    fn context(&self, parameters: &Parameters) -> Result<(), Error> {
        if parameters.len() > 16
            || !parameters.iter().all(|(k, v)| {
                token(k) && token(v) && self.axes.get(k).is_some_and(|values| values.contains(v))
            })
        {
            return Err(Error::Context);
        }
        Ok(())
    }
}
/// Complete project context validated before projection; not deserializable.
pub struct Context {
    space: Object,
    parameters: Parameters,
    object: Object,
    bytes: Vec<u8>,
}
impl Context {
    pub fn space(&self) -> &Object {
        &self.space
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn parameters(&self) -> &Parameters {
        &self.parameters
    }
    pub fn restore(pin: &Object, raw: &[u8], space: &Space) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            schema: String,
            space: Object,
            parameters: Parameters,
        }
        let wire: Wire = decode(pin, raw)?;
        if wire.schema != "tig.variant-context.experimental-1" || &wire.space != space.object() {
            return Err(Error::Context);
        }
        let result = space.admit(&wire.parameters)?;
        if result.object() != pin {
            return Err(Error::Pin);
        }
        Ok(result)
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    parameters: Parameters,
    slot: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexWire {
    schema: String,
    asset: String,
    definition: Reference,
    space: Object,
    semantics: Digest,
    uses: Vec<String>,
    entries: Vec<Entry>,
}
pub struct Index {
    object: Object,
    wire: IndexWire,
    uses: BTreeSet<String>,
}
impl Index {
    pub fn parse(pin: &Object, raw: &[u8]) -> Result<Self, Error> {
        let wire: IndexWire = decode(pin, raw)?;
        let uses: BTreeSet<_> = wire.uses.iter().cloned().collect();
        if wire.schema != "tig.variant-index.experimental-1"
            || wire.asset.is_empty()
            || wire.asset.len() > 256
            || wire.asset.chars().any(char::is_control)
            || wire.definition.validate().is_err()
            || wire.space.bytes > MAX_RECORD_BYTES as u64
            || wire.uses.len() > 16
            || uses.len() != wire.uses.len()
            || !uses.iter().all(|k| token(k))
            || wire.entries.is_empty()
            || wire.entries.len() > 256
        {
            return Err(Error::Index);
        }
        let mut sizes = BTreeMap::new();
        for p in [pin, &wire.space, &wire.definition.object] {
            if sizes
                .insert(&p.sha256, p.bytes)
                .is_some_and(|n| n != p.bytes)
            {
                return Err(Error::Pin);
            }
        }
        let mut seen = BTreeSet::new();
        for entry in &wire.entries {
            if entry.slot.is_empty()
                || entry.slot.len() > 128
                || entry.slot.chars().any(char::is_control)
                || entry.parameters.keys().cloned().collect::<BTreeSet<_>>() != uses
                || !entry.parameters.values().all(|v| token(v))
                || !seen.insert(&entry.parameters)
            {
                return Err(Error::Index);
            }
        }
        Ok(Self {
            object: pin.clone(),
            wire,
            uses,
        })
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
}

/// Must come from caller policy, not copied out of an untrusted index.
pub struct Admission {
    pub definition: Reference,
    pub space: Object,
    pub semantics: Digest,
    pub uses: BTreeSet<String>,
}
pub struct Selection {
    checkpoint: Digest,
    record: Reference,
    index: Object,
    space: Object,
    context_space: Object,
    context: Object,
    parameters: Parameters,
    slot: String,
    role: String,
    group: String,
    object: Object,
    subject: Digest,
}
impl Selection {
    pub fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub fn record(&self) -> &Reference {
        &self.record
    }
    pub fn index(&self) -> &Object {
        &self.index
    }
    pub fn space(&self) -> &Object {
        &self.space
    }
    pub fn context_space(&self) -> &Object {
        &self.context_space
    }
    pub fn context(&self) -> &Object {
        &self.context
    }
    pub fn parameters(&self) -> &Parameters {
        &self.parameters
    }
    pub fn slot(&self) -> &str {
        &self.slot
    }
    pub fn role(&self) -> &str {
        &self.role
    }
    pub fn group(&self) -> &str {
        &self.group
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn subject(&self) -> &Digest {
        &self.subject
    }
    pub fn controls(&self) -> BTreeSet<Object> {
        BTreeSet::from([
            self.index.clone(),
            self.space.clone(),
            self.context_space.clone(),
            self.context.clone(),
        ])
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
}
impl std::fmt::Debug for Selection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VariantSelection")
            .field("axes", &self.parameters.len())
            .field("object", &self.object)
            .finish_non_exhaustive()
    }
}

pub fn resolve(
    record: &Resolved,
    admission: &Admission,
    space: &Space,
    index: &Index,
    context: &Parameters,
) -> Result<Selection, Error> {
    let context = space.admit(context)?;
    resolve_in_context(record, admission, space, index, &context, space.object())
}

/// A project context may add unused dimensions/values without rewriting neutral
/// assets. Both its space pin and each asset's original space are caller admitted.
pub fn resolve_in_context(
    record: &Resolved,
    admission: &Admission,
    space: &Space,
    index: &Index,
    context: &Context,
    admitted_context_space: &Object,
) -> Result<Selection, Error> {
    if context.space() != admitted_context_space {
        return Err(Error::Admission);
    }
    if record.definition() != &admission.definition
        || index.wire.definition != admission.definition
        || index.wire.asset != record.reference().id
        || space.object != admission.space
        || index.wire.space != admission.space
        || index.wire.semantics != admission.semantics
        || index.uses != admission.uses
        || !index.uses.iter().all(|k| space.axes.contains_key(k))
    {
        return Err(Error::Admission);
    }
    if record.selected().map_err(|_| Error::Missing)? != &index.object {
        return Err(Error::Pin);
    }
    for entry in &index.wire.entries {
        space.context(&entry.parameters)?;
        record.stream(&entry.slot).map_err(|_| Error::Stream)?;
    }
    let parameters = index
        .uses
        .iter()
        .map(|axis| {
            context
                .parameters
                .get(axis)
                .cloned()
                .map(|v| (axis.clone(), v))
                .ok_or(Error::Context)
        })
        .collect::<Result<Parameters, _>>()?;
    space.context(&parameters)?;
    let entry = index
        .wire
        .entries
        .iter()
        .find(|e| e.parameters == parameters)
        .ok_or(Error::Missing)?;
    let stream = record
        .stream_bindings()
        .iter()
        .find(|s| s.slot == entry.slot)
        .ok_or(Error::Stream)?;
    let object = stream.object.clone();
    let subject=Digest::blob(&canonical_json(&json!({"schema":"tig.variant-selection.experimental-1",
        "asset":record.reference().id,"definition":admission.definition,"semantics":admission.semantics,
        "parameters":parameters,"slot":entry.slot,"role":stream.role,"group":stream.group,"object":object})).map_err(|_|Error::Json)?);
    Ok(Selection {
        checkpoint: record.checkpoint().clone(),
        record: record.reference().clone(),
        index: index.object.clone(),
        space: space.object.clone(),
        context_space: context.space.clone(),
        context: context.object.clone(),
        parameters,
        slot: entry.slot.clone(),
        role: stream.role.clone(),
        group: stream.group.clone(),
        object,
        subject,
    })
}
