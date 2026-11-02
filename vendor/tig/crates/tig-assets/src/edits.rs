//! Pure expected-base source edits. No persistence, approval or custody authority.
use crate::{
    delivery,
    native::{Instance, Pin},
    revisions::{self, Coverage, Limits, Reader},
    schema::{Dialect, Schema},
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use tig_core::{Object, identity};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Stale,
    Native,
    Schema,
    Conflict,
    Unconfigured,
    Selection,
    Budget,
    Revision(revisions::Error),
    Delivery(delivery::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

pub struct Inputs<'a> {
    pub expected: &'a Object,
    pub instance: &'a [u8],
    pub definition: &'a [u8],
    pub authored_schema: &'a [u8],
    pub dialect: Dialect,
    pub limits: Limits,
}
pub enum Change<'a> {
    ConfigureDelivery {
        policy: &'a [u8],
    },
    RegisterRevision {
        manifest: &'a [u8],
        coverage: Coverage,
    },
    RecordDelivery {
        event: &'a [u8],
        coverage: Coverage,
    },
    SelectRevision {
        revision: &'a Object,
        representation: &'a str,
        subject: &'a Pin,
        status: &'a Map<String, Value>,
        streams: &'a [&'a str],
    },
}
/// Only new immutable metadata, never a copy of all prior ledgers or media.
pub struct Blob {
    object: Object,
    bytes: Vec<u8>,
}
impl Blob {
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
pub struct Prepared {
    expected: Object,
    record: Blob,
    metadata: Vec<Blob>,
    noop: bool,
}
impl Prepared {
    pub fn expected(&self) -> &Object {
        &self.expected
    }
    pub fn record(&self) -> &Blob {
        &self.record
    }
    pub fn metadata(&self) -> &[Blob] {
        &self.metadata
    }
    pub fn is_noop(&self) -> bool {
        self.noop
    }
    pub fn retained(&self) -> bool {
        false
    }
    pub fn local_contracts_validated(&self) -> bool {
        false
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
    pub fn authorizes_eviction(&self) -> bool {
        false
    }
}

struct Memo<'a, R> {
    source: &'a mut R,
    limits: Limits,
    bytes: usize,
    cached: BTreeMap<Object, Vec<u8>>,
    supplied: BTreeMap<Object, Vec<u8>>,
}
impl<R: Reader> Reader for Memo<'_, R> {
    fn read(
        &mut self,
        object: &Object,
        maximum: usize,
    ) -> std::result::Result<Vec<u8>, revisions::Error> {
        if object.bytes > maximum as u64 {
            return Err(revisions::Error::Budget);
        }
        if let Some(raw) = self.cached.get(object) {
            return Ok(raw.clone());
        }
        if self.cached.len() >= self.limits.documents
            || object.bytes > self.limits.bytes.saturating_sub(self.bytes) as u64
        {
            return Err(revisions::Error::Budget);
        }
        let raw = match self.supplied.get(object) {
            Some(raw) => raw.clone(),
            None => self.source.read(object, maximum)?,
        };
        if raw.len() as u64 != object.bytes || Object::from_bytes(&raw) != *object {
            return Err(revisions::Error::Pin);
        }
        self.bytes += raw.len();
        self.cached.insert(object.clone(), raw.clone());
        Ok(raw)
    }
}
fn supplied<R>(memo: &mut Memo<'_, R>, blobs: &mut Vec<Blob>, bytes: Vec<u8>) -> Result<Object> {
    if bytes.len() > 65_536 {
        return Err(Error::Budget);
    }
    identity::parse_json(&bytes).map_err(|_| Error::Native)?;
    let object = Object::from_bytes(&bytes);
    memo.supplied.insert(object.clone(), bytes.clone());
    if !blobs.iter().any(|b| b.object == object) {
        blobs.push(Blob {
            object: object.clone(),
            bytes,
        });
    }
    Ok(object)
}
fn cap<'a>(record: &'a Value, name: &str) -> Option<&'a Map<String, Value>> {
    record["metadata"].get(name)?.as_object()
}
fn slot(record: &Value, name: &str, field: &str) -> Option<String> {
    cap(record, name)?.get(field)?.as_str().map(str::to_owned)
}
fn stream_object(record: &Value, name: &str) -> Result<Object> {
    let stream = record["streams"]
        .as_array()
        .ok_or(Error::Native)?
        .iter()
        .find(|s| s["slot"] == name)
        .ok_or(Error::Native)?;
    let pin: Pin = serde_json::from_value(stream["object"].clone()).map_err(|_| Error::Native)?;
    Ok(pin.object())
}
fn set_stream(
    record: &mut Value,
    name: &str,
    role: &str,
    object: &Object,
    owned: bool,
) -> Result<()> {
    let streams = record["streams"].as_array_mut().ok_or(Error::Native)?;
    if let Some(stream) = streams.iter_mut().find(|s| s["slot"] == name) {
        if !owned || stream["role"] != role {
            return Err(Error::Conflict);
        }
        let pin = stream["object"].as_object_mut().ok_or(Error::Native)?;
        pin.insert(
            "sha256".into(),
            serde_json::to_value(&object.sha256).map_err(|_| Error::Native)?,
        );
        pin.insert("bytes".into(), json!(object.bytes));
    } else {
        streams.push(json!({"slot":name,"role":role,"group":"history","object":object}));
    }
    Ok(())
}
fn ensure_cap<'a>(
    record: &'a mut Value,
    name: &str,
    schema: &str,
) -> Result<&'a mut Map<String, Value>> {
    let metadata = record["metadata"].as_object_mut().ok_or(Error::Native)?;
    if !metadata.contains_key(name) {
        metadata.insert(name.into(), json!({"schema":schema}));
    }
    metadata
        .get_mut(name)
        .and_then(Value::as_object_mut)
        .ok_or(Error::Native)
}
fn coverage(value: Coverage) -> &'static str {
    match value {
        Coverage::Complete => "complete",
        Coverage::Partial => "partial",
    }
}

fn status_bounds(status: &Map<String, Value>) -> Result<()> {
    if status.len() > 4096 {
        return Err(Error::Budget);
    }
    let mut bytes = 0usize;
    let mut nodes = 1usize;
    let mut stack = Vec::new();
    for (key, value) in status {
        bytes = bytes.saturating_add(key.len());
        stack.push((value, 1));
    }
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        if depth > 32 || nodes > 4096 || bytes > 65_536 {
            return Err(Error::Budget);
        }
        match value {
            Value::String(text) => bytes = bytes.saturating_add(text.len()),
            Value::Array(array) => {
                if nodes + stack.len() + array.len() > 4096 {
                    return Err(Error::Budget);
                }
                stack.extend(array.iter().map(|v| (v, depth + 1)));
            }
            Value::Object(map) => {
                if nodes + stack.len() + map.len() > 4096 {
                    return Err(Error::Budget);
                }
                for (key, child) in map {
                    bytes = bytes.saturating_add(key.len());
                    stack.push((child, depth + 1));
                }
            }
            _ => (),
        }
    }
    if bytes > 65_536 {
        return Err(Error::Budget);
    }
    identity::canonical_json(&Value::Object(status.clone())).map_err(|_| Error::Native)?;
    Ok(())
}

/// Prepared bytes must be explicitly retained and base-fenced by existing tools.
/// A successful source edit is never approval, a Git commit or a storage action.
pub fn prepare(
    inputs: Inputs<'_>,
    change: Change<'_>,
    reader: &mut impl Reader,
) -> Result<Prepared> {
    if [
        inputs.instance.len(),
        inputs.definition.len(),
        inputs.authored_schema.len(),
    ]
    .into_iter()
    .any(|n| n > 65_536)
    {
        return Err(Error::Budget);
    }
    if inputs.expected != &Object::from_bytes(inputs.instance) {
        return Err(Error::Stale);
    }
    match &change {
        Change::ConfigureDelivery { policy }
        | Change::RegisterRevision {
            manifest: policy, ..
        }
        | Change::RecordDelivery { event: policy, .. } => {
            if policy.len() > 65_536 {
                return Err(Error::Budget);
            }
        }
        Change::SelectRevision {
            representation,
            subject,
            status,
            streams,
            ..
        } => {
            revisions::token(representation).map_err(Error::Revision)?;
            if subject
                .id
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
                || subject
                    .media_type
                    .as_ref()
                    .is_some_and(|s| s.len() > 128 || s.chars().any(char::is_control))
                || subject.bytes > 9_007_199_254_740_991
            {
                return Err(Error::Selection);
            }
            status_bounds(status)?;
            if streams.len() > 256 {
                return Err(Error::Budget);
            }
            let mut names = std::collections::BTreeSet::new();
            for name in *streams {
                revisions::token(name).map_err(Error::Revision)?;
                if name.len() > 128 || !names.insert(name) {
                    return Err(Error::Selection);
                }
            }
        }
    }
    let original = Instance::parse(inputs.instance).map_err(|_| Error::Native)?;
    let bound = original
        .bind(inputs.definition, inputs.authored_schema)
        .map_err(|_| Error::Native)?;
    let engine = Schema::compile(bound.definition(), inputs.authored_schema, inputs.dialect)
        .map_err(|_| Error::Schema)?;
    let checked = engine.check(&bound).map_err(|_| Error::Schema)?;
    revisions::Budget::new(&bound, &checked, inputs.limits).map_err(Error::Revision)?;
    let mut memo = Memo {
        source: reader,
        limits: inputs.limits,
        bytes: 0,
        cached: BTreeMap::new(),
        supplied: BTreeMap::new(),
    };
    let before_delivery = if bound.instance().value().metadata.contains_key("delivery") {
        delivery::check(&bound, &checked, inputs.limits, &mut memo).map_err(Error::Delivery)?
    } else {
        None
    };
    let before_revisions = if before_delivery.is_none() {
        revisions::check(&bound, &checked, inputs.limits, &mut memo).map_err(Error::Revision)?
    } else {
        None
    };
    let current_revisions = before_delivery
        .as_ref()
        .and_then(|h| h.revisions())
        .or(before_revisions.as_ref());
    let mut record = identity::parse_json(inputs.instance).map_err(|_| Error::Native)?;
    let source_value = record.clone();
    let mut blobs = Vec::new();
    match change {
        Change::ConfigureDelivery { policy } => {
            let object = supplied(&mut memo, &mut blobs, policy.to_vec())?;
            let existing = slot(&record, "delivery", "policy_slot");
            let name = existing.as_deref().unwrap_or("tig-delivery-policy");
            set_stream(
                &mut record,
                name,
                "policy/delivery",
                &object,
                existing.is_some(),
            )?;
            ensure_cap(&mut record, "delivery", "tig.delivery-state.experimental-1")?
                .insert("policy_slot".into(), json!(name));
        }
        Change::RegisterRevision {
            manifest,
            coverage: declared,
        } => {
            let object = supplied(&mut memo, &mut blobs, manifest.to_vec())?;
            if !current_revisions.is_some_and(|h| h.contains_revision(&object)) {
                let existing = slot(&record, "revisions", "history_slot");
                let parents = existing
                    .as_deref()
                    .map(|s| stream_object(&record, s))
                    .transpose()?
                    .into_iter()
                    .collect::<Vec<_>>();
                let page = json!({"schema":"tig.revision-history.experimental-1","asset":bound.instance().value().id,
                    "coverage":coverage(declared),"parents":parents,"revisions":[object]});
                let root = supplied(
                    &mut memo,
                    &mut blobs,
                    identity::canonical_json(&page).map_err(|_| Error::Native)?,
                )?;
                let name = existing.as_deref().unwrap_or("tig-revision-history");
                set_stream(
                    &mut record,
                    name,
                    "history/revisions",
                    &root,
                    existing.is_some(),
                )?;
                ensure_cap(
                    &mut record,
                    "revisions",
                    "tig.revision-state.experimental-1",
                )?
                .insert("history_slot".into(), json!(name));
            }
        }
        Change::RecordDelivery {
            event,
            coverage: declared,
        } => {
            if before_delivery.is_none() {
                return Err(Error::Unconfigured);
            }
            let object = supplied(&mut memo, &mut blobs, event.to_vec())?;
            if !before_delivery
                .as_ref()
                .is_some_and(|h| h.contains_event(&object))
            {
                let existing = slot(&record, "delivery", "history_slot");
                let parents = existing
                    .as_deref()
                    .map(|s| stream_object(&record, s))
                    .transpose()?
                    .into_iter()
                    .collect::<Vec<_>>();
                let page = json!({"schema":"tig.delivery-history.experimental-1","asset":bound.instance().value().id,
                    "coverage":coverage(declared),"parents":parents,"events":[object]});
                let root = supplied(
                    &mut memo,
                    &mut blobs,
                    identity::canonical_json(&page).map_err(|_| Error::Native)?,
                )?;
                let name = existing.as_deref().unwrap_or("tig-delivery-history");
                set_stream(
                    &mut record,
                    name,
                    "history/delivery",
                    &root,
                    existing.is_some(),
                )?;
                ensure_cap(&mut record, "delivery", "tig.delivery-state.experimental-1")?
                    .insert("history_slot".into(), json!(name));
            }
        }
        Change::SelectRevision {
            revision,
            representation,
            subject,
            status,
            streams,
        } => {
            let history = current_revisions.ok_or(Error::Selection)?;
            if history.representation_pin(revision, representation) != Some(&subject.object()) {
                return Err(Error::Selection);
            }
            if history.selected_revision().is_none()
                && bound
                    .instance()
                    .value()
                    .selected
                    .as_ref()
                    .is_some_and(|old| old.object() != subject.object())
            {
                return Err(Error::Selection);
            }
            for name in streams {
                let stream = bound
                    .instance()
                    .value()
                    .streams
                    .iter()
                    .find(|s| s.slot == *name)
                    .ok_or(Error::Selection)?;
                if matches!(
                    stream.role.as_str(),
                    "history/revisions"
                        | "revision/selected"
                        | "history/delivery"
                        | "policy/delivery"
                ) {
                    return Err(Error::Selection);
                }
                let expected = bound
                    .instance()
                    .value()
                    .selected
                    .as_ref()
                    .map(|p| p.object())
                    .unwrap_or_else(|| subject.object());
                if stream.object.object() != expected && stream.object.object() != subject.object()
                {
                    return Err(Error::Selection);
                }
                let target = record["streams"]
                    .as_array_mut()
                    .ok_or(Error::Native)?
                    .iter_mut()
                    .find(|s| s["slot"] == *name)
                    .ok_or(Error::Selection)?;
                target["object"] = serde_json::to_value(subject).map_err(|_| Error::Native)?;
            }
            let existing = cap(&record, "revisions")
                .and_then(|c| c.get("selection"))
                .and_then(|s| s.get("slot"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let name = existing.as_deref().unwrap_or("tig-selected-revision");
            set_stream(
                &mut record,
                name,
                "revision/selected",
                revision,
                existing.is_some(),
            )?;
            ensure_cap(
                &mut record,
                "revisions",
                "tig.revision-state.experimental-1",
            )?
            .insert(
                "selection".into(),
                json!({"slot":name,"representation":representation}),
            );
            record["selected"] = serde_json::to_value(subject).map_err(|_| Error::Native)?;
            record["status"] = Value::Object(status.clone());
        }
    }
    if record == source_value {
        return Ok(Prepared {
            expected: inputs.expected.clone(),
            record: Blob {
                object: inputs.expected.clone(),
                bytes: inputs.instance.to_vec(),
            },
            metadata: vec![],
            noop: true,
        });
    }
    let bytes = identity::canonical_json(&record).map_err(|_| Error::Native)?;
    let candidate = Instance::parse(&bytes).map_err(|_| Error::Native)?;
    let bound = candidate
        .bind(inputs.definition, inputs.authored_schema)
        .map_err(|_| Error::Native)?;
    let checked = engine.check(&bound).map_err(|_| Error::Schema)?;
    if bound.instance().value().metadata.contains_key("delivery") {
        delivery::check(&bound, &checked, inputs.limits, &mut memo).map_err(Error::Delivery)?;
    } else {
        revisions::check(&bound, &checked, inputs.limits, &mut memo).map_err(Error::Revision)?;
    }
    Ok(Prepared {
        expected: inputs.expected.clone(),
        record: Blob {
            object: Object::from_bytes(&bytes),
            bytes,
        },
        metadata: blobs,
        noop: false,
    })
}
