//! Explicit publication of new prepared metadata, never full graph custody.
use std::io::Cursor;
use tig_assets::edits::Prepared;
use tig_core::{Digest, LogicalPath, Object, identity};
use tig_store::{AckLevel, Key, PublicationBackend, Receipt, Request};

use crate::source::proposal::Replacement;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Budget,
    Conflict,
    Integrity,
    Unsupported,
    Busy,
    Provider,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
fn provider(error: tig_store::Error) -> Error {
    match error {
        tig_store::Error::InvalidRequest => Error::Invalid,
        tig_store::Error::Budget => Error::Budget,
        tig_store::Error::OperationConflict => Error::Conflict,
        tig_store::Error::Unsupported => Error::Unsupported,
        tig_store::Error::Busy | tig_store::Error::Retained => Error::Busy,
        tig_store::Error::Integrity
        | tig_store::Error::CorruptRecord
        | tig_store::Error::MissingAcknowledgedObject => Error::Integrity,
        _ => Error::Provider,
    }
}

pub struct Published {
    expected: Object,
    record: Object,
    bytes: Vec<u8>,
    intent: Option<Object>,
    receipts: Vec<Receipt>,
}
impl Published {
    pub fn intent(&self) -> Option<&Object> {
        self.intent.as_ref()
    }
    /// Acknowledgements of the intent and new objects, never complete graph custody.
    pub fn receipts(&self) -> &[Receipt] {
        &self.receipts
    }
    pub fn record(&self) -> &Object {
        &self.record
    }
    pub fn is_noop(&self) -> bool {
        self.intent.is_none()
    }
    pub fn replacement(&self, path: LogicalPath) -> Replacement {
        Replacement {
            path,
            expected: self.expected.clone(),
            bytes: self.bytes.clone(),
        }
    }
    pub fn independent_custody_verified(&self) -> bool {
        false
    }
    pub fn authorizes_old_protection_release(&self) -> bool {
        false
    }
}
fn key(operation: &Key, suffix: &str) -> Result<Key, Error> {
    let identity = identity::canonical_json(&serde_json::json!([operation.as_str(), suffix]))
        .map_err(|_| Error::Invalid)?;
    Key::parse(&format!("history-{}", Digest::blob(&identity).as_str())).map_err(provider)
}

/// Caller must retain old roots; success only acknowledges newly prepared bytes.
pub fn publish(
    prepared: &Prepared,
    operation: &Key,
    owner: &Key,
    required: AckLevel,
    backend: &mut impl PublicationBackend,
) -> Result<Published, Error> {
    let mut result = Published {
        expected: prepared.expected().clone(),
        record: prepared.record().object().clone(),
        bytes: prepared.record().bytes().to_vec(),
        intent: None,
        receipts: Vec::new(),
    };
    if prepared.is_noop() {
        return Ok(result);
    }
    if prepared.metadata().len() > 256 {
        return Err(Error::Budget);
    }
    let metadata: Vec<_> = prepared.metadata().iter().map(|b| b.object()).collect();
    let raw = identity::canonical_json(&serde_json::json!({
        "schema":"tig.history-update-intent.experimental-1",
        "operation":operation,"owner":owner,"required":required,
        "expected":prepared.expected(),"record":prepared.record().object(),"metadata":metadata
    }))
    .map_err(|_| Error::Invalid)?;
    if raw.len() > 65_536 {
        return Err(Error::Budget);
    }
    let intent = Object::from_bytes(&raw);
    let mut requests = vec![(
        Request {
            operation: key(operation, "intent")?,
            owner: owner.clone(),
            object: intent.clone(),
            required,
        },
        raw.as_slice(),
    )];
    for (index, blob) in prepared
        .metadata()
        .iter()
        .chain([prepared.record()])
        .enumerate()
    {
        requests.push((
            Request {
                operation: key(operation, &format!("object-{index}"))?,
                owner: owner.clone(),
                object: blob.object().clone(),
                required,
            },
            blob.bytes(),
        ));
    }
    let caps = backend.capabilities();
    for (request, _) in &requests {
        caps.check(request).map_err(provider)?;
    }
    for (request, bytes) in requests {
        let receipt =
            tig_store::publish(backend, &request, &mut Cursor::new(bytes)).map_err(provider)?;
        result.receipts.push(receipt);
    }
    result.intent = Some(intent);
    Ok(result)
}
