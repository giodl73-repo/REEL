//! Explicit issuer-profile context. Claim metadata reuse does not execute issuance.
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{Digest, Object, identity::canonical_json};
use tig_store::{
    Key,
    local::LocalStore,
    provider::{custody::create, profile},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issuance {
    pub operation: Key,
    pub profiles: BTreeMap<Digest, profile::Profile>,
    pub bindings: BTreeMap<String, Digest>,
}
pub struct Keys {
    pub claim: Key,
    pub budget: Key,
    pub finish: Key,
}
#[derive(Clone)]
pub struct Accepted {
    pub expected: Digest,
    pub profile: profile::Profile,
}
impl Issuance {
    pub fn validate(&self, places: &[String]) -> Result<(), String> {
        if self.profiles.len() > 16
            || self.bindings.len() > 16
            || self.bindings.keys().collect::<BTreeSet<_>>()
                != places.iter().collect::<BTreeSet<_>>()
        {
            return Err("issuer profile aliases conflict with selected scope".into());
        }
        let used = self.bindings.values().cloned().collect::<BTreeSet<_>>();
        if self.profiles.keys().cloned().collect::<BTreeSet<_>>() != used {
            return Err("missing or unused issuer profile".into());
        }
        for (id, p) in &self.profiles {
            if p.fingerprint().map_err(|_| "invalid issuer profile")? != *id {
                return Err("issuer profile fingerprint conflict".into());
            }
        }
        Ok(())
    }
    pub fn select(
        &self,
        aliases: &[String],
        place: &Digest,
        work: &Digest,
        owner: &Key,
        object: &Object,
    ) -> Result<(Accepted, Keys), String> {
        let ids = aliases
            .iter()
            .map(|alias| {
                self.bindings
                    .get(alias)
                    .ok_or("missing selected issuer profile")
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if ids.len() != 1 {
            return Err("aliased issuer profiles conflict".into());
        }
        let id = (*ids.first().ok_or("empty selected issuer aliases")?).clone();
        let p = self.profiles.get(&id).ok_or("missing issuer record")?;
        if p.binding.place != *place {
            return Err("issuer profile place conflict".into());
        }
        let bytes = canonical_json(&serde_json::json!([
            "tig.profiled-retained-claim.experimental-1",
            self.operation,
            owner,
            work,
            place,
            object,
            id
        ]))
        .map_err(|_| "issuer operation identity budget exceeded")?;
        let digest = Digest::blob(&bytes);
        let key = |prefix: &str| {
            Key::parse(&format!("{prefix}{}", digest.as_str()))
                .map_err(|_| "invalid profiled retention key".to_string())
        };
        Ok((
            Accepted {
                expected: id,
                profile: p.clone(),
            },
            Keys {
                claim: key("claim-")?,
                budget: key("claim-budget-")?,
                finish: key("claim-finished-")?,
            },
        ))
    }
}
impl Accepted {
    pub fn preflight(&self, store: &LocalStore) -> Result<(), String> {
        self.profile
            .admit(&self.expected, &self.profile.binding)
            .and_then(|guard| guard.check_issue(&create::native::capabilities(store)))
            .map_err(|error| format!("issuer profile refused ({error})"))
    }
    pub fn issue<I: create::Issuer>(
        &self,
        issuer: &mut I,
        request: &create::Request,
    ) -> Result<create::Issued<I::Evidence>, create::Error> {
        let guard = self
            .profile
            .admit(&self.expected, &self.profile.binding)
            .map_err(|_| create::Error::Stale)?;
        guard.issue(issuer, request, true)
    }
}
