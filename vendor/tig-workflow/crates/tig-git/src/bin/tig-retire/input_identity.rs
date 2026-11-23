//! Immutable admission identity excludes producer paths and advancing witnesses.
use super::{codec, model::StageInput};
use serde_json::Value;
use tig_core::Digest;

pub fn fingerprint(input: &StageInput) -> Result<Digest, String> {
    input.validate()?;
    let mut value = serde_json::to_value(input).map_err(|_| "invalid stage identity")?;
    let object = value.as_object_mut().ok_or("invalid stage identity")?;
    let authorities = object
        .get_mut("authorities")
        .and_then(Value::as_object_mut)
        .ok_or("invalid authority identity")?;
    for authority in authorities.values_mut() {
        authority
            .as_object_mut()
            .ok_or("invalid authority")?
            .remove("minimum_checkpoint");
    }
    for place in object
        .get_mut("places")
        .and_then(Value::as_object_mut)
        .ok_or("invalid place identity")?
        .values_mut()
    {
        place
            .get_mut("capacity")
            .and_then(Value::as_object_mut)
            .ok_or("invalid capacity identity")?
            .remove("minimum_checkpoint");
    }
    for field in ["review", "context"] {
        let artifact = object.get_mut(field).ok_or("missing artifact")?;
        *artifact = artifact.get("object").cloned().ok_or("invalid artifact")?;
    }
    let grants = object
        .get_mut("grants")
        .and_then(Value::as_array_mut)
        .ok_or("invalid grants")?;
    for grant in grants.iter_mut() {
        *grant = grant.get("object").cloned().ok_or("invalid grant")?;
    }
    // Ordering of declarative inventories does not change the admitted recipe.
    // Duplicates remain visible and must be refused by exact scope admission.
    for field in ["grants", "preservations", "handoffs", "history"] {
        let array = object
            .get_mut(field)
            .and_then(Value::as_array_mut)
            .ok_or("invalid inventory")?;
        let mut encoded = array
            .iter()
            .map(|v| codec::encode(v).map(|bytes| (bytes, v.clone())))
            .collect::<Result<Vec<_>, _>>()?;
        encoded.sort_by(|left, right| left.0.cmp(&right.0));
        *array = encoded.into_iter().map(|(_, value)| value).collect();
    }
    Ok(Digest::blob(&codec::encode(&(
        "tig.native-retirement-input.experimental-1",
        value,
    ))?))
}
