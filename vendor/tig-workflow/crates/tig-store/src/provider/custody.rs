//! Held exact retained custody. No local paths, readers or removal capability.
use crate::{AckLevel, Key, acquire::SourceError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tig_core::{Digest, Object, identity};
pub mod create;
pub mod native;
pub mod release;
pub const MAX_SURVIVORS: usize = 16;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Commitment {
    pub place: Digest,
    pub object: Object,
    pub claim: Digest,
    pub owner: Key,
    pub level: AckLevel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Enforcement {
    Cooperative,
    ProviderHeld,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub place: Digest,
    pub max_object_bytes: u64,
    pub max_ack: AckLevel,
    pub enforcement: Option<Enforcement>,
}
impl Capabilities {
    pub(super) fn check(&self, wanted: &Commitment, cooperative: bool) -> Result<(), SourceError> {
        if self.place != wanted.place {
            return Err(SourceError::Stale);
        }
        if wanted.object.bytes > self.max_object_bytes {
            return Err(SourceError::Budget);
        }
        if wanted.level > self.max_ack
            || self.enforcement.is_none()
            || (self.enforcement == Some(Enforcement::Cooperative) && !cooperative)
        {
            return Err(SourceError::Unsupported);
        }
        Ok(())
    }
}
/// Non-serializable effective hold. The exact verified bytes AND active claim
/// must remain protected against release/removal until drop. A timestamp or
/// expiring observation alone cannot implement this contract. Failed validation
/// must keep prior protection held while making the lease unusable for removal.
pub trait Held {
    fn commitment(&self) -> &Commitment;
    fn revalidate(&mut self) -> Result<(), SourceError>;
    fn fresh(&self) -> Result<(), SourceError>;
}
/// Trusted admitted provider. Acquisition verifies the exact active commitment
/// and sufficient integrity/persistence evidence, then obtains an effective hold.
/// No download, filesystem, publish, release or deletion method is required.
pub trait Retainer {
    fn describe(&self) -> Capabilities;
    fn acquire(&mut self, wanted: &Commitment) -> Result<Box<dyn Held + '_>, SourceError>;
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Survivor {
    pub commitment: Commitment,
    pub failure_domain: Key,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub source_domain: Key,
    pub retained: Vec<Survivor>,
    pub required_domains: usize,
    pub minimum_ack: AckLevel,
    pub allow_cooperative: bool,
}
impl Profile {
    pub fn places(
        &self,
        source: &Digest,
        object: &Object,
    ) -> Result<BTreeSet<Digest>, SourceError> {
        if self.retained.is_empty()
            || self.retained.len() > MAX_SURVIVORS
            || self.required_domains == 0
            || self.required_domains > MAX_SURVIVORS
            || self.minimum_ack < AckLevel::ProcessRestart
        {
            return Err(SourceError::Invalid);
        }
        let mut places = BTreeSet::new();
        let mut domains = BTreeSet::new();
        let mut previous = None;
        for s in &self.retained {
            let c = &s.commitment;
            if c.place == *source
                || c.object != *object
                || c.level < self.minimum_ack
                || previous.is_some_and(|p: &Digest| p >= &c.place)
            {
                return Err(SourceError::Invalid);
            }
            previous = Some(&c.place);
            places.insert(c.place.clone());
            if s.failure_domain != self.source_domain {
                domains.insert(s.failure_domain.as_str());
            }
        }
        if domains.len() < self.required_domains {
            return Err(SourceError::Invalid);
        }
        Ok(places)
    }
    pub fn digest(&self) -> Result<Digest, SourceError> {
        if self.retained.len() > MAX_SURVIVORS {
            return Err(SourceError::Budget);
        }
        let value = serde_json::to_value(("tig.provider-custody-profile.design-1", self))
            .map_err(|_| SourceError::Invalid)?;
        Ok(Digest::blob(
            &identity::canonical_json(&value).map_err(|_| SourceError::Invalid)?,
        ))
    }
}
pub struct Group<'a> {
    keepers: Vec<(Commitment, Box<dyn Held + 'a>)>,
    valid: bool,
}
impl Group<'_> {
    pub fn revalidate(&mut self) -> Result<(), SourceError> {
        self.valid = false;
        for (wanted, held) in &mut self.keepers {
            held.revalidate()?;
            if held.commitment() != wanted {
                return Err(SourceError::Stale);
            }
        }
        self.fresh_inner()?;
        self.valid = true;
        Ok(())
    }
    fn fresh_inner(&self) -> Result<(), SourceError> {
        for (wanted, held) in &self.keepers {
            if held.commitment() != wanted {
                return Err(SourceError::Stale);
            }
            held.fresh()?;
        }
        Ok(())
    }
    pub fn fresh(&self) -> Result<(), SourceError> {
        if !self.valid {
            return Err(SourceError::Stale);
        }
        self.fresh_inner()
    }
}
pub fn acquire<'a>(
    profile: &Profile,
    source: &Digest,
    object: &Object,
    endpoints: &'a mut [&mut dyn Retainer],
) -> Result<Group<'a>, SourceError> {
    let places = profile.places(source, object)?;
    if endpoints.len() != profile.retained.len() {
        return Err(SourceError::Invalid);
    }
    let mut ids = BTreeSet::new();
    // Check every capability before the first provider acquisition.
    let profiles = endpoints.iter().map(|p| p.describe()).collect::<Vec<_>>();
    for p in &profiles {
        if !ids.insert(p.place.clone()) {
            return Err(SourceError::Invalid);
        }
        let c = profile
            .retained
            .iter()
            .find(|s| s.commitment.place == p.place)
            .ok_or(SourceError::Invalid)?;
        p.check(&c.commitment, profile.allow_cooperative)?;
    }
    if ids != places {
        return Err(SourceError::Invalid);
    }
    let mut keepers = Vec::with_capacity(endpoints.len());
    for (endpoint, described) in endpoints.iter_mut().zip(profiles) {
        if endpoint.describe() != described {
            return Err(SourceError::Stale);
        }
        let wanted = profile
            .retained
            .iter()
            .find(|s| s.commitment.place == described.place)
            .ok_or(SourceError::Invalid)?
            .commitment
            .clone();
        let held = endpoint.acquire(&wanted)?;
        if held.commitment() != &wanted {
            return Err(SourceError::Stale);
        }
        held.fresh()?;
        keepers.push((wanted, held));
    }
    let group = Group {
        keepers,
        valid: true,
    };
    group.fresh()?;
    Ok(group)
}
