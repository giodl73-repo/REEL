//! Explicit native-annex reads. No retrieval, publication or custody inference.
use super::{VerifiedCopy, hold_copy_with_guards, info, invalid, managed_guards, uuid};
use crate::annex::{
    identity::Key,
    process::{Enrollment, Runner},
};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Seek};
use tig_core::{Digest, Object};
use tig_store::source_claims::protection::effects::HeldObject;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub pool: Enrollment,
    pub store: Digest,
    pub maximum_bytes: u64,
}

/// A verified, rewound stream with mutation/namespace and enrolled object gates
/// held for its lifetime. Windows local profile only; not retained custody.
pub struct VerifiedRead {
    copy: VerifiedCopy,
    _admission: Vec<HeldObject>,
}
impl Read for VerifiedRead {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.copy._file.read(buffer)
    }
}

pub fn open(profile: &Profile, object: &Object) -> io::Result<VerifiedRead> {
    if !cfg!(windows)
        || profile.maximum_bytes == 0
        || profile.maximum_bytes > 64 * 1024 * 1024 * 1024
        || object.bytes > profile.maximum_bytes
    {
        return Err(invalid());
    }
    let runner = Runner::new(profile.pool.clone())?;
    if uuid(&info(&runner, None)?)?.store() != profile.store {
        return Err(invalid());
    }
    let key = Key::from_object(object);
    let admission = managed_guards(&[&runner], &key)?;
    let mut copy = hold_copy_with_guards(
        &runner,
        &key,
        profile.maximum_bytes,
        crate::annex::legacy::hold_link_directories,
    )?
    .ok_or_else(invalid)?;
    copy._file.rewind()?;
    Ok(VerifiedRead {
        copy,
        _admission: admission,
    })
}

/// Normalize only the native provider's already validated CurDir prefix. Asset
/// LogicalPath rules remain unchanged; native namespaces can include .git/.tig.
pub(super) fn provider_location(
    copy: &VerifiedCopy,
) -> io::Result<tig_store::existing::ProviderPath> {
    let native = copy.location.replace('\\', "/");
    let relative = native
        .split('/')
        .filter(|part| *part != ".")
        .collect::<Vec<_>>()
        .join("/");
    tig_store::existing::ProviderPath::parse(&relative).map_err(|_| invalid())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationRequest {
    pub schema: String,
    pub source: Profile,
    pub object: Object,
}
/// Exact read-only payload observation for a caller that will immediately open
/// a guarded provider-file reader and revalidate. The receipt alone is no lease.
pub fn write_location(
    request: &LocationRequest,
    profile: Object,
    output: &std::path::Path,
) -> io::Result<serde_json::Value> {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        path::Component,
    };
    if request.schema != "tig.annex-read-location.experimental-1"
        || !output.is_absolute()
        || output
            .components()
            .any(|p| matches!(p, Component::ParentDir))
        || fs::symlink_metadata(output).is_ok()
    {
        return Err(invalid());
    }
    let parent = fs::canonicalize(output.parent().ok_or_else(invalid)?)?;
    let _parents = crate::annex::legacy::hold_link_directories(&parent)?;
    for root in [
        Some(&request.source.pool.repository),
        Some(&request.source.pool.empty_hooks),
        request.source.pool.control.as_ref().map(|p| &p.root),
    ]
    .into_iter()
    .flatten()
    {
        if parent.starts_with(root) || fs::canonicalize(root).is_ok_and(|p| parent.starts_with(p)) {
            return Err(invalid());
        }
    }
    let guard = open(&request.source, &request.object)?;
    let value = serde_json::json!({"schema":"tig.annex-read-location-report.experimental-1","profile":profile,
        "store":request.source.store,"object":request.object,"private_location":provider_location(&guard.copy)?,"payload_verified":true,"custody":false});
    let bytes = serde_json::to_vec(&value).map_err(|_| invalid())?;
    if bytes.len() > 65536 {
        return Err(invalid());
    }
    let attempt = super::adoption::stage(&parent)?;
    let _attempt = crate::annex::legacy::hold_link_directories(&attempt)?;
    let temporary = attempt.join("receipt.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::hard_link(temporary, output)?;
    Ok(value)
}
