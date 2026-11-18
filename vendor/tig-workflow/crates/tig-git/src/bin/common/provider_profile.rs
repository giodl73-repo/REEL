//! Opt-in native publication profile context, bound into both operation IDs.
use serde::{Deserialize, Serialize};
use tig_core::{Digest, Object, identity::canonical_json};
use tig_store::{Key, Request, provider::profile};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub profile: profile::Profile,
    pub expected: Digest,
    pub binding: profile::Binding,
    pub operation: Key,
    pub inspection_digest: Digest,
}

pub fn keys(
    operation: &Key,
    owner: &Key,
    inspection: &Digest,
    place: &Digest,
    object: &Object,
    expected: &Digest,
) -> Result<(Key, Key), String> {
    let bytes = canonical_json(&serde_json::json!([
        "tig.profiled-publication-job.experimental-1",
        operation,
        owner,
        inspection,
        place,
        object,
        expected
    ]))
    .map_err(|_| "profiled operation identity budget exceeded")?;
    let digest = Digest::blob(&bytes);
    Ok((
        Key::parse(&format!("copy-{}", digest.as_str()))
            .map_err(|_| "invalid profiled publication ID")?,
        Key::parse(&format!("admit-{}", digest.as_str()))
            .map_err(|_| "invalid profiled admission ID")?,
    ))
}
impl Envelope {
    pub fn validate(&self, place: &Digest, request: &Request, job: &Key) -> Result<(), String> {
        if self
            .profile
            .fingerprint()
            .map_err(|_| "invalid provider profile")?
            != self.expected
            || self.binding != self.profile.binding
            || self.binding.place != *place
            || request.required != tig_store::AckLevel::ProcessRestart
        {
            return Err("provider profile fingerprint, binding or acknowledgement conflict".into());
        }
        let (publication, admission) = keys(
            &self.operation,
            &request.owner,
            &self.inspection_digest,
            place,
            &request.object,
            &self.expected,
        )?;
        if request.operation != publication || *job != admission {
            return Err("profiled publication/admission identity conflict".into());
        }
        Ok(())
    }
}
