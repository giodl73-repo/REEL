//! Lower explicit Tig bindings into existing delivery controls, without media IO.
//! This is preparation, not planning, execution, retention or freshness authority.
use crate::{
    cue_relative,
    scene_delivery::{FileRef, Job},
    tig_inputs::Source,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{Digest, Object};

pub const SCHEMA: &str = "reel.tig-delivery-template.experimental-1";
mod context;
pub use context::{freeze_context, recipe, requirements, restore_context};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub root_id: String,
    pub root_definition: String,
    pub primary_definitions: BTreeSet<String>,
    pub control_definitions: BTreeSet<String>,
    #[serde(default)]
    pub authored_control_fields: BTreeMap<String, String>,
}
/// Private operational controls. No paths are created and no bytes are protected.
pub struct Prepared {
    pub source_checkpoint: Digest,
    pub job: Job,
    pub contract_bytes: Vec<u8>,
    pub production_bytes: Vec<u8>,
    pub consumed_bindings: BTreeMap<String, FileRef>,
}
fn token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
fn references(value: &Value) -> Result<BTreeMap<String, String>> {
    let map = value
        .as_object()
        .context("reference map must be an object")?;
    if map.is_empty() || map.len() > 4096 {
        bail!("reference map exceeds bounds");
    }
    map.iter()
        .map(|(alias, value)| {
            let r = value.as_object().context("reference must be an object")?;
            let id = r
                .get("id")
                .and_then(Value::as_str)
                .context("reference id missing")?;
            if !token(alias) || !token(id) || r.len() != 1 {
                bail!("invalid reference");
            }
            Ok((alias.clone(), id.to_owned()))
        })
        .collect()
}
fn selected(status: &Map<String, Value>) -> Result<()> {
    if !matches!(
        status.get("selection_state").and_then(Value::as_str),
        Some("selected-private-production" | "principal-approved" | "release-cleared")
    ) {
        bail!("consumed record is not selected for production");
    }
    Ok(())
}
fn bounded(value: &Value) -> Result<Vec<u8>> {
    let raw = serde_json::to_vec(value)?;
    if raw.len() > crate::tig_inputs::JOB_BYTES_MAX {
        bail!("control document exceeds bounds");
    }
    Ok(raw)
}
fn expand(
    value: &mut Value,
    source: &Source,
    aliases: &BTreeMap<String, String>,
    consumed: &mut BTreeMap<String, FileRef>,
) -> Result<()> {
    let r = value.as_object().context("input must name a binding")?;
    let alias = r
        .get("binding")
        .and_then(Value::as_str)
        .context("input binding missing")?;
    if r.len() != 1 {
        bail!("input binding has conflicting fields");
    }
    let id = aliases
        .get(alias)
        .context("input binding is not declared")?;
    let leaf = source.record(id)?;
    selected(leaf.status())?;
    let asset = source.resolve_asset(id)?;
    tig_core::LogicalPath::parse(id)?;
    let file = FileRef {
        path: id.into(),
        sha256: asset.sha256,
        bytes: asset.bytes,
    };
    consumed.insert(alias.into(), file.clone());
    *value = serde_json::to_value(file)?;
    Ok(())
}
/// Resolve all record types before interpreting controls. Unused alternatives
/// may remain draft, but outputs/control types cannot masquerade as primaries.
/// Request definition admission is trusted policy, not independent schema proof.
impl Request {
    pub(crate) fn validate(&self) -> Result<()> {
        let request = self;
        let all = request
            .primary_definitions
            .iter()
            .chain(&request.control_definitions);
        if request.schema != SCHEMA
            || !token(&request.root_id)
            || !token(&request.root_definition)
            || request.primary_definitions.is_empty()
            || request.primary_definitions.len() > 256
            || request.control_definitions.is_empty()
            || request.control_definitions.len() > 64
            || all
                .clone()
                .any(|id| !token(id) || id == &request.root_definition)
            || !request
                .primary_definitions
                .is_disjoint(&request.control_definitions)
            || request
                .authored_control_fields
                .iter()
                .any(|(id, field)| !request.control_definitions.contains(id) || !token(field))
        {
            bail!("invalid delivery template admission");
        }
        Ok(())
    }
}
/// Lower one exact admitted snapshot; no source mutation or media reads.
pub fn lower(source: &mut Source, request: &Request) -> Result<Prepared> {
    request.validate()?;
    let parent = source.record(&request.root_id)?;
    if parent.definition().id != request.root_definition {
        bail!("root definition differs");
    }
    selected(parent.status())?;
    if parent.authored().contains_key("language_binding_ids") {
        bail!("delivery template variants require explicit resolved admission");
    }
    let aliases = references(
        parent
            .authored()
            .get("binding_ids")
            .context("bindings missing")?,
    )?;
    for id in aliases.values() {
        if !request
            .primary_definitions
            .contains(&source.record(id)?.definition().id)
        {
            bail!("binding is outside admitted primary definitions");
        }
    }
    let controls = references(
        parent
            .authored()
            .get("control_ids")
            .context("controls missing")?,
    )?;
    if controls.keys().map(String::as_str).collect::<BTreeSet<_>>()
        != BTreeSet::from(["contract", "job", "production"])
    {
        bail!("exact job, contract and production controls required");
    }
    let mut documents = BTreeMap::new();
    let mut control_bytes = 0u64;
    for (name, id) in controls {
        let control = source.record(&id)?;
        if !request
            .control_definitions
            .contains(&control.definition().id)
        {
            bail!("control definition differs");
        }
        selected(control.status())?;
        let prepared = crate::tig_scene::controls::prepare(
            source,
            &control,
            &request.authored_control_fields,
        )?;
        control_bytes = control_bytes
            .checked_add(prepared.pin.bytes)
            .context("control byte overflow")?;
        if control_bytes > 16 * 1024 * 1024 {
            bail!("controls exceed aggregate bounds");
        }
        let raw = if prepared.authored.is_some() {
            prepared.bytes
        } else {
            source.read_metadata(&prepared.pin, crate::tig_inputs::JOB_BYTES_MAX)?
        };
        crate::approval_attestation::ensure_no_duplicate_keys(&raw)?;
        let doc: Value = serde_json::from_slice(&raw)?;
        if !doc.is_object() {
            bail!("control document must be an object");
        }
        documents.insert(name, doc);
    }
    let production_bytes = bounded(&documents["production"])?;
    let contract: cue_relative::Contract = serde_json::from_value(documents["contract"].clone())?;
    if contract.production_manifest != std::path::Path::new("production.json") {
        bail!("production virtual name differs");
    }
    // Existing compiler owns clock schemas, namespaces, spans and arithmetic.
    cue_relative::compile_with_manifest_bytes(&contract, &production_bytes)?;
    let contract_bytes = bounded(&documents["contract"])?;
    let mut job = documents.remove("job").unwrap();
    if job["schema"] != SCHEMA
        || job.get("contract").is_some()
        || job.get("production_manifest_sha256").is_some()
    {
        bail!("job template has conflicting generated controls");
    }
    let mut consumed = BTreeMap::new();
    for field in ["pictures", "audio", "external_layers"] {
        let Some(values) = job.get_mut(field) else {
            continue;
        };
        let entries = values
            .as_array_mut()
            .context("delivery collection must be an array")?;
        if entries.len() > 4096 {
            bail!("delivery collection exceeds bounds");
        }
        for entry in entries {
            let fields: &[&str] = if field == "external_layers" {
                &["evidence", "font", "render_source", "derivation_receipt"]
            } else {
                &["source"]
            };
            for key in fields {
                if let Some(value) = entry.get_mut(*key).filter(|v| !v.is_null()) {
                    expand(value, source, &aliases, &mut consumed)?;
                }
            }
            if field == "external_layers" {
                if let Some(frames) = entry.get_mut("frames") {
                    let frames = frames.as_array_mut().context("frames must be an array")?;
                    if frames.len() > 4096 {
                        bail!("sequence frames exceed bounds");
                    }
                    for frame in frames {
                        expand(frame, source, &aliases, &mut consumed)?;
                    }
                }
            }
            if field == "audio" {
                if let Some(value) = entry.pointer_mut("/placement_offset/evidence") {
                    expand(value, source, &aliases, &mut consumed)?;
                }
            }
        }
    }
    if let Some(value) = job.pointer_mut("/post_compose_camera/evidence") {
        expand(value, source, &aliases, &mut consumed)?;
    }
    job["schema"] = json!("reel.scene-delivery.v0.1");
    let pin = Object::from_bytes(&contract_bytes);
    job["contract"] = json!({"path":"contract.json","sha256":pin.sha256,"bytes":pin.bytes});
    job["production_manifest_sha256"] = json!(Object::from_bytes(&production_bytes).sha256);
    if serde_json::to_vec(&job)?.len() > crate::tig_inputs::JOB_BYTES_MAX {
        bail!("lowered job exceeds bounds");
    }
    let job: Job = serde_json::from_value(job)?;
    // A timed-overlay receipt owns more exact references than its top-level
    // render source. Validate them against the same primary catalog; never let
    // the lower-level provider's wider definition admission bypass this policy.
    let mut metadata_bytes = 0u64;
    let alias_by_id = aliases
        .iter()
        .map(|(alias, id)| (id.as_str(), alias))
        .collect::<BTreeMap<_, _>>();
    for layer in &job.external_layers {
        if let Some(receipt) = &layer.derivation_receipt {
            metadata_bytes = metadata_bytes
                .checked_add(receipt.bytes)
                .context("metadata overflow")?;
            if metadata_bytes > 16 * 1024 * 1024 {
                bail!("derivation metadata exceeds bounds");
            }
            let object = Object {
                sha256: Digest::parse(&receipt.sha256)?,
                bytes: receipt.bytes,
            };
            let raw = source.read_metadata(&object, crate::tig_inputs::JOB_BYTES_MAX)?;
            crate::approval_attestation::ensure_no_duplicate_keys(&raw)?;
            let derived: crate::scene_delivery::TimedOverlayDerivation =
                serde_json::from_slice(&raw)?;
            if derived.schema != "reel.timed-overlay-derivation.v1"
                || derived.selected_evidence != layer.evidence
                || layer.render_source.as_ref() != Some(&derived.output)
                || derived.inputs.is_empty()
                || derived.inputs.len() > 4096
                || !derived.recipe.is_object()
            {
                bail!("derivation does not bind selected layer");
            }
            for expected in &derived.inputs {
                let id = expected
                    .path
                    .to_str()
                    .context("derivation input id is not UTF-8")?;
                let alias = alias_by_id
                    .get(id)
                    .context("derivation input is outside primary bindings")?;
                let mut deferred = json!({"binding":alias});
                expand(&mut deferred, source, &aliases, &mut consumed)?;
                if serde_json::from_value::<FileRef>(deferred)? != *expected {
                    bail!("derivation input pin is stale");
                }
            }
        } else if layer.render_source.is_some() {
            bail!("derived render source requires its exact receipt");
        }
    }
    Ok(Prepared {
        source_checkpoint: source.checkpoint().clone(),
        job,
        contract_bytes,
        production_bytes,
        consumed_bindings: consumed,
    })
}
