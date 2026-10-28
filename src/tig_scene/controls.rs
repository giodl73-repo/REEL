//! Explicit independently versioned controls in the same admitted Git snapshot.
use super::*;
use tig_assets::resolve::Resolved;

pub(super) struct Catalog {
    pub ids: BTreeMap<String, String>,
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
fn object(source: &Source, record: &Resolved) -> Result<Object> {
    Ok(match &source.admission(&record.definition().id)?.payload {
        Payload::Selected {} => record.selected()?,
        Payload::Stream { slot } => record.stream(slot)?,
    }
    .clone())
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
                let pin = object(source, &record)?;
                // No shadow authority: a retained alias must describe this revision.
                if let Ok(old) = parent.stream(slot) {
                    if old != &pin {
                        bail!("parent control stream differs from current control reference");
                    }
                }
                ids.insert(slot.clone(), id.into());
            }
        }
        Ok(Self { ids })
    }
    pub fn object(&self, source: &Source, parent: &Resolved, slot: &str) -> Result<Object> {
        let Some(id) = self.ids.get(slot) else {
            return Ok(parent.stream(slot)?.clone());
        };
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
        object(source, &record)
    }
}
