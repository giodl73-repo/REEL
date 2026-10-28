//! Explicit independently versioned controls in the same admitted Git snapshot.
use super::*;
use tig_assets::resolve::Resolved;

pub(super) struct Catalog {
    pub ids: BTreeMap<String, String>,
    fields: BTreeMap<String, String>,
}
pub(super) struct Materialized {
    pub bytes: Vec<u8>,
    pub pin: Object,
    pub authored: Option<Value>,
}
pub(super) fn rule() -> Rule {
    Rule {
        pointer: "/control_ids".into(),
        kind: EdgeKind::Production,
        collection: Collection::ObjectValues,
        optional: false,
        binding: Binding::SnapshotSelected,
    }
}
pub(super) fn payloads(payload: &Payload) -> Payloads {
    match payload {
        Payload::Selected {} => Payloads::SelectedObject,
        Payload::Stream { slot } => Payloads::Named {
            selected: false,
            slots: BTreeSet::from([slot.clone()]),
        },
    }
}
fn prepare(
    source: &Source,
    record: &Resolved,
    fields: &BTreeMap<String, String>,
) -> Result<Materialized> {
    if let Some(field) = fields.get(&record.definition().id) {
        let value = record
            .authored()
            .get(field)
            .filter(|v| v.is_object())
            .context("authored control field must be a JSON object")?;
        let bytes = tig_core::identity::canonical_json(value)?;
        if bytes.len() > crate::tig_inputs::JOB_BYTES_MAX {
            bail!("authored control exceeds metadata budget");
        }
        let pin = Object::from_bytes(&bytes);
        return Ok(Materialized {
            bytes,
            authored: Some(json!({"record":record.reference(),"field":field,
                "canonical_json":pin,"stored_byte_object":false})),
            pin,
        });
    }
    let pin = match &source.admission(&record.definition().id)?.payload {
        Payload::Selected {} => record.selected()?,
        Payload::Stream { slot } => record.stream(slot)?,
    }
    .clone();
    Ok(Materialized {
        bytes: Vec::new(),
        pin,
        authored: None,
    })
}
impl Catalog {
    pub fn read(source: &Source, parent: &Resolved, request: &Request) -> Result<Self> {
        let mut ids = BTreeMap::new();
        if let Some(value) = parent.authored().get("control_ids") {
            let refs = value.as_object().context("control_ids must be an object")?;
            if refs.is_empty() || refs.len() > 128 {
                bail!("native control count exceeds budget");
            }
            for (slot, value) in refs {
                let reference = value.as_object().context("invalid control reference")?;
                let id = reference
                    .get("id")
                    .and_then(Value::as_str)
                    .context("control id missing")?;
                if slot.is_empty()
                    || slot.len() > 256
                    || slot.chars().any(char::is_control)
                    || reference.len() != 1
                    || id.is_empty()
                    || id.len() > 256
                    || id.chars().any(char::is_control)
                    || request.controls.template_receipt.as_ref() == Some(slot)
                {
                    bail!("unsupported native control reference");
                }
                let record = source.record(id)?;
                if !request
                    .control_definitions
                    .contains(&record.definition().id)
                {
                    bail!("control is outside admitted control definitions");
                }
                let pin = prepare(source, &record, &request.authored_control_fields)?.pin;
                // No shadow authority: a retained alias must describe this revision.
                if let Ok(old) = parent.stream(slot) {
                    if old != &pin {
                        bail!("parent control stream differs from current control reference");
                    }
                }
                ids.insert(slot.clone(), id.into());
            }
        }
        Ok(Self {
            ids,
            fields: request.authored_control_fields.clone(),
        })
    }
    pub fn materialize(
        &self,
        source: &mut Source,
        parent: &Resolved,
        slot: &str,
        total: &mut u64,
    ) -> Result<Materialized> {
        let mut control = if let Some(id) = self.ids.get(slot) {
            let record = source.record(id)?;
            if !matches!(
                record
                    .status()
                    .get("selection_state")
                    .and_then(Value::as_str),
                Some("selected-private-production" | "principal-approved" | "release-cleared")
            ) {
                bail!("native consumed control is not selected for production");
            }
            prepare(source, &record, &self.fields)?
        } else {
            Materialized {
                bytes: Vec::new(),
                pin: parent.stream(slot)?.clone(),
                authored: None,
            }
        };
        if control.authored.is_some() {
            *total = total
                .checked_add(control.pin.bytes)
                .context("metadata size overflow")?;
            if *total > METADATA_MAX {
                bail!("native scene metadata exceeds aggregate budget");
            }
        } else {
            control.bytes = metadata(source, &control.pin, total)?;
        }
        Ok(control)
    }
}
