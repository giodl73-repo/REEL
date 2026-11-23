//! Explicit native initialization in an owned private stage; no resume-time setup.
use crate::{
    Key,
    ledger::{CapacityLedger, Checkpoint},
    local::{self, LocalStore, StoreProfile},
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tig_core::{Digest, identity};
mod namespace;
use namespace::{Context, present};
pub const INPUT_SCHEMA: &str = "tig.local-enrollment-profile.experimental-1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub root: PathBuf,
    pub operation: Key,
    pub place: Key,
    pub profile: StoreProfile,
    pub capacity: Capacity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Capacity {
    New {
        domain: Key,
        epoch: Key,
        floor: u64,
    },
    Existing {
        root: PathBuf,
        ledger: Digest,
        minimum_checkpoint: Checkpoint,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub root: PathBuf,
    pub place: Digest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityBinding {
    pub root: PathBuf,
    pub ledger: Digest,
    pub minimum_checkpoint: Checkpoint,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub schema: String,
    pub operation: Key,
    pub intent: Digest,
    pub destination: Destination,
    pub capacity: CapacityBinding,
    pub profile: StoreProfile,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    intent: Digest,
    store: Digest,
    capacity: CapacityBinding,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Conflict,
    Busy,
    PendingInitialization,
    Io,
    Injected,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}
type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    IntentPinned,
    StoreReady,
    CapacityReady,
    RecordReady,
    Published,
}

pub fn initialize(request: &Request) -> Result<Enrollment> {
    initialize_hook(request, |_| Ok(()))
}
fn initialize_hook(
    request: &Request,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Enrollment> {
    let context = Context::prepare(request)?;
    if present(&context.request.root)? {
        return published(&context);
    }
    let _fence = context.fence()?;
    if present(&context.request.root)? {
        return published(&context);
    }
    hook(Stage::IntentPinned)?;
    context.prepare_stage()?;
    let store_path = context.stage.join("store");
    let store = if present(&store_path)? {
        LocalStore::open(&store_path).map_err(|_| Error::PendingInitialization)?
    } else {
        LocalStore::create_profile(
            &store_path,
            context.request.place.clone(),
            context.request.profile.clone(),
        )
        .map_err(|_| Error::PendingInitialization)?
    };
    check_store(&context.request, &store)?;
    store
        .check_unpublished_empty()
        .map_err(|_| Error::PendingInitialization)?;
    hook(Stage::StoreReady)?;
    let capacity = prepare_capacity(&context)?;
    hook(Stage::CapacityReady)?;
    let record = Record {
        schema: "tig.local-enrollment-record.experimental-1".into(),
        intent: context.intent.clone(),
        store: store.identity().clone(),
        capacity,
    };
    context.write_record(&record)?;
    drop(store);
    hook(Stage::RecordReady)?;
    // Same-parent publication, serialized only with cooperating initializers of
    // this exact target. Hostile parent/path mutation is outside native assurance.
    if present(&context.request.root)? {
        return Err(Error::Conflict);
    }
    let resolved_stage = fs::canonicalize(&context.stage)?;
    if resolved_stage.parent() != context.request.root.parent() {
        return Err(Error::Conflict);
    }
    fs::rename(resolved_stage, &context.request.root)?;
    hook(Stage::Published)?;
    published(&context)
}
fn check_store(request: &Request, store: &LocalStore) -> Result<()> {
    if store.place() != &request.place
        || store.max_object_bytes() != request.profile.max_object_bytes
        || store.removal_enabled() != request.profile.removable
    {
        return Err(Error::Conflict);
    }
    Ok(())
}
fn prepare_capacity(context: &Context) -> Result<CapacityBinding> {
    match &context.request.capacity {
        Capacity::New {
            domain,
            epoch,
            floor,
        } => {
            let path = context.stage.join("capacity");
            let ledger = if present(&path)? {
                CapacityLedger::reopen_unpublished_empty(&path, domain, epoch, *floor)
            } else {
                CapacityLedger::create(&path, domain.clone(), epoch.clone(), *floor)
            }
            .map_err(|_| Error::PendingInitialization)?;
            Ok(CapacityBinding {
                root: context.request.root.join("capacity"),
                ledger: ledger.identity().clone(),
                minimum_checkpoint: ledger.checkpoint(),
            })
        }
        Capacity::Existing {
            root,
            ledger,
            minimum_checkpoint,
        } => {
            let opened =
                CapacityLedger::open(root, minimum_checkpoint).map_err(|_| Error::Conflict)?;
            if opened.identity() != ledger {
                return Err(Error::Conflict);
            }
            Ok(CapacityBinding {
                root: root.clone(),
                ledger: ledger.clone(),
                minimum_checkpoint: opened.checkpoint(),
            })
        }
    }
}
fn published(context: &Context) -> Result<Enrollment> {
    local::directory(&context.request.root).map_err(|_| Error::Conflict)?;
    let record: Record = local::read_record(&context.request.root.join("enrollment.json"))
        .map_err(|_| Error::Conflict)?;
    if record.schema != "tig.local-enrollment-record.experimental-1"
        || record.intent != context.intent
    {
        return Err(Error::Conflict);
    }
    let store =
        LocalStore::open(&context.request.root.join("store")).map_err(|_| Error::Conflict)?;
    check_store(&context.request, &store)?;
    if store.identity() != &record.store {
        return Err(Error::Conflict);
    }
    let expected_root = match &context.request.capacity {
        Capacity::New { .. } => context.request.root.join("capacity"),
        Capacity::Existing { root, .. } => root.clone(),
    };
    if record.capacity.root != expected_root {
        return Err(Error::Conflict);
    }
    let mut ledger = CapacityLedger::open(&expected_root, &record.capacity.minimum_checkpoint)
        .map_err(|_| Error::Conflict)?;
    if ledger.identity() != &record.capacity.ledger {
        return Err(Error::Conflict);
    }
    if let Capacity::Existing {
        ledger: expected,
        minimum_checkpoint,
        ..
    } = &context.request.capacity
    {
        if expected != ledger.identity() {
            return Err(Error::Conflict);
        }
        ledger = CapacityLedger::open(&expected_root, minimum_checkpoint)
            .map_err(|_| Error::Conflict)?;
    }
    Ok(Enrollment {
        schema: "tig.local-enrollment.experimental-1".into(),
        operation: context.request.operation.clone(),
        intent: context.intent.clone(),
        destination: Destination {
            root: context.request.root.join("store"),
            place: record.store,
        },
        capacity: CapacityBinding {
            minimum_checkpoint: ledger.checkpoint(),
            ..record.capacity
        },
        profile: context.request.profile.clone(),
    })
}
#[cfg(test)]
mod tests;
