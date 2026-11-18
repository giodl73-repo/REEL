use super::model::{self, Artifact, Handle};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::{fs::File, io::Read};
use tig_core::{
    Digest, Object,
    identity::{canonical_json, parse_json},
    tree::NodeStore,
};
use tig_store::metadata::MetadataStore;

pub fn encode(value: &impl Serialize) -> Result<Vec<u8>, String> {
    canonical_json(&serde_json::to_value(value).map_err(|_| "invalid native control record")?)
        .map_err(|_| "native control record budget".into())
}
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    if bytes.len() > model::MAX_ARTIFACT as usize {
        return Err("native control input byte budget".into());
    }
    serde_json::from_value(
        parse_json(bytes).map_err(|_| "invalid/over-budget native control JSON")?,
    )
    .map_err(|_| "invalid native control fields".into())
}
pub fn get(nodes: &impl NodeStore, object: &Object) -> Result<Vec<u8>, String> {
    if object.bytes == 0 || object.bytes > model::MAX_ARTIFACT {
        return Err("native artifact bounds".into());
    }
    let bytes = nodes
        .get(object)
        .map_err(|_| "native artifact unavailable")?
        .ok_or("native artifact missing")?;
    if Object::from_bytes(&bytes) != *object {
        return Err("native artifact integrity".into());
    }
    Ok(bytes)
}
pub fn put(nodes: &mut MetadataStore, bytes: &[u8]) -> Result<Object, String> {
    if bytes.is_empty() || bytes.len() > model::MAX_ARTIFACT as usize {
        return Err("native artifact byte budget".into());
    }
    let object = Object::from_bytes(bytes);
    if nodes
        .put(bytes)
        .map_err(|_| "native artifact write refused; effects may exist")?
        != object
        || get(nodes, &object)? != bytes
    {
        return Err("native artifact acknowledgement/integrity".into());
    }
    Ok(object)
}
pub struct ArtifactBudget {
    used: u64,
}
impl ArtifactBudget {
    pub fn new() -> Self {
        Self { used: 0 }
    }
    pub fn load(
        &mut self,
        nodes: &mut MetadataStore,
        artifact: &Artifact,
    ) -> Result<Vec<u8>, String> {
        if artifact.object.bytes == 0
            || artifact.object.bytes > model::MAX_ARTIFACT
            || !artifact.file.is_absolute()
        {
            return Err("native control artifact bounds/binding".into());
        }
        self.used = self
            .used
            .checked_add(artifact.object.bytes)
            .ok_or("native artifact aggregate budget")?;
        if self.used > model::MAX_CONTROL_BYTES {
            return Err("native artifact aggregate budget".into());
        }
        if let Some(bytes) = nodes
            .get(&artifact.object)
            .map_err(|_| "native artifact metadata unavailable")?
        {
            if Object::from_bytes(&bytes) != artifact.object {
                return Err("stored control artifact integrity".into());
            }
            return Ok(bytes);
        }
        let mut bytes = Vec::new();
        File::open(&artifact.file)
            .map_err(|_| "explicit control artifact unavailable")?
            .take(artifact.object.bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "cannot read explicit control artifact")?;
        if Object::from_bytes(&bytes) != artifact.object {
            return Err("explicit control artifact changed".into());
        }
        put(nodes, &bytes)?;
        Ok(bytes)
    }
}
pub fn handle_digest(handle: &Handle) -> Result<Digest, String> {
    if handle.schema == model::POLICY_HANDLE {
        return Ok(Digest::blob(&encode(&(
            model::POLICY_HANDLE,
            &handle.operation,
            &handle.recipe,
            &handle.enrollment,
            &handle.authorities,
            &handle.capacity_minimums,
            &handle.policy_minimum,
        ))?));
    }
    Ok(Digest::blob(&encode(&(
        model::HANDLE,
        &handle.operation,
        &handle.recipe,
        &handle.enrollment,
        &handle.authorities,
        &handle.capacity_minimums,
    ))?))
}
pub fn handle(bytes: &[u8]) -> Result<Handle, String> {
    let handle: Handle = decode(bytes)?;
    handle.authorities.validate()?;
    let raw = parse_json(bytes).map_err(|_| "invalid work encoding")?;
    if (handle.schema == model::HANDLE && raw.get("policy_minimum").is_some())
        || !matches!(
            (handle.schema.as_str(), handle.policy_minimum.as_ref()),
            (model::HANDLE, None) | (model::POLICY_HANDLE, Some(_))
        )
        || handle.recipe.bytes == 0
        || handle.recipe.bytes > model::MAX_ARTIFACT
        || handle.capacity_minimums.len() > 16
        || handle_digest(&handle)? != handle.digest
    {
        return Err("invalid native retirement work identity/bounds".into());
    }
    Ok(handle)
}
