use super::*;
use std::fs::File;

pub(super) fn slot_key(slot: &Key) -> super::super::Result<Key> {
    Key::parse(&format!(
        "policy-slot-{}",
        Digest::blob(&encode(&(RECORD, slot))?).as_str()
    ))
    .map_err(|_| Error::InvalidReference)
}
pub(super) fn command_key(
    registry: &Digest,
    slot: &Key,
    operation: &Key,
    phase: &str,
) -> RegistryResult<Key> {
    Key::parse(&format!(
        "policy-{}",
        Digest::blob(
            &encode(&(RECORD, registry, slot, operation, phase)).map_err(RegistryError::Journal)?
        )
        .as_str()
    ))
    .map_err(|_| RegistryError::Encoding)
}
pub(super) fn check_ref(object: &Object, maximum: u64) -> RegistryResult<()> {
    if object.bytes == 0 || object.bytes > maximum {
        return Err(RegistryError::Encoding);
    }
    Ok(())
}
pub(super) fn read(
    nodes: &impl NodeStore,
    object: &Object,
    maximum: u64,
) -> RegistryResult<Vec<u8>> {
    check_ref(object, maximum)?;
    let bytes = nodes
        .get(object)
        .map_err(|_| RegistryError::Metadata)?
        .ok_or(RegistryError::Metadata)?;
    if Object::from_bytes(&bytes) != *object {
        return Err(RegistryError::Encoding);
    }
    Ok(bytes)
}
pub(super) fn read_registration(
    nodes: &impl NodeStore,
    root: &Object,
) -> RegistryResult<Registration> {
    let bytes = read(nodes, root, 1024)?;
    let record: Registration =
        serde_json::from_value(identity::parse_json(&bytes).map_err(|_| RegistryError::Encoding)?)
            .map_err(|_| RegistryError::Encoding)?;
    if record.bytes()? != bytes {
        return Err(RegistryError::Encoding);
    }
    Ok(record)
}
pub(super) fn load_registration(
    nodes: &impl NodeStore,
    binding: &Binding,
    record: &Registration,
) -> RegistryResult<(Policy, ProviderCatalogue)> {
    let policy = Policy::from_json(&read(nodes, &record.policy, 65_536)?)
        .map_err(|_| RegistryError::Encoding)?;
    binding.check_policy(&policy)?;
    let catalogue = ProviderCatalogue::from_json(&read(nodes, &record.providers, 65_536)?)?;
    if policy.config().provider_bindings != record.providers.sha256 {
        return Err(RegistryError::Encoding);
    }
    for place in catalogue.places().keys() {
        if !binding.stores.contains(place) {
            return Err(RegistryError::Scope);
        }
    }
    Ok((policy, catalogue))
}

pub(super) struct SlotGuard(File);
impl SlotGuard {
    pub(super) fn acquire(root: &Path, key: &Key, shared: bool) -> RegistryResult<Self> {
        local::directory(&root.join("fences")).map_err(|_| RegistryError::Metadata)?;
        let path = root.join("fences").join(key.as_str());
        match fs::symlink_metadata(&path) {
            Ok(_) => local::regular(&path).map_err(|_| RegistryError::Metadata)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(RegistryError::Metadata),
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| RegistryError::Metadata)?;
        if file.metadata().map_err(|_| RegistryError::Metadata)?.len() != 0 {
            return Err(RegistryError::Encoding);
        }
        let result = if shared {
            file.try_lock_shared()
        } else {
            file.try_lock()
        };
        result.map_err(|e| {
            RegistryError::Journal(match e {
                std::fs::TryLockError::WouldBlock => Error::Busy,
                _ => Error::Io,
            })
        })?;
        Ok(Self(file))
    }
}
impl Drop for SlotGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
