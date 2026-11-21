//! Explicit admitted application decision; parsing does not authenticate an owner.
use super::{Loss, Release, Review};
use crate::{Digest, Object, identity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub object: Object,
    pub store: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consent {
    pub release_owned_claims: BTreeSet<Release>,
    pub remove_copies: BTreeSet<Target>,
    pub accept_intentional_loss: BTreeSet<Object>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub namespace: Digest,
    pub operation: Digest,
    pub owner: Digest,
    pub evidence: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Budget,
    Empty,
    Owner,
    Scope,
    Held,
    LossConsent,
    Encoding,
    Review(super::Error),
}
type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Decision {
    schema: &'static str,
    scope: &'static str,
    reviewed: Digest,
    approval: Approval,
    consent: Consent,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_context: Option<Digest>,
}
impl Decision {
    /// The caller must authenticate/admit the owner evidence. No implicit policy consent.
    pub fn authorize(review: &Review, approval: Approval, consent: Consent) -> Result<Self> {
        if consent.release_owned_claims.len() > super::MAX_OBLIGATIONS
            || consent.remove_copies.len() > super::MAX_CELLS
            || consent.accept_intentional_loss.len() > crate::retirement::MAX_SELECTED
        {
            return Err(Error::Budget);
        }
        if review.is_routine_eviction() && !consent.accept_intentional_loss.is_empty() {
            return Err(Error::LossConsent);
        }
        if approval.owner != review.owner {
            return Err(Error::Owner);
        }
        let objects: BTreeSet<_> = consent
            .remove_copies
            .iter()
            .map(|t| t.object.clone())
            .collect();
        if objects.is_empty() {
            return Err(Error::Empty);
        }
        if objects.len() > crate::retirement::MAX_SELECTED {
            return Err(Error::Budget);
        }
        let mut targets = BTreeSet::new();
        let mut loss = BTreeSet::new();
        for object in &objects {
            let entry = review
                .objects
                .iter()
                .find(|o| &o.object == object)
                .ok_or(Error::Scope)?;
            if !entry.eligible() {
                return Err(Error::Held);
            }
            targets.extend(entry.targets.iter().map(|store| Target {
                object: object.clone(),
                store: store.clone(),
            }));
            match &entry.loss {
                Loss::NoObservedSurvivor => {
                    loss.insert(object.clone());
                }
                Loss::ObservedSurvivors(_) => (),
                Loss::Unknown => return Err(Error::Held),
            }
        }
        let releases: BTreeSet<_> = review
            .requested_releases
            .iter()
            .filter(|r| objects.contains(&r.object))
            .cloned()
            .collect();
        if targets != consent.remove_copies || releases != consent.release_owned_claims {
            return Err(Error::Scope);
        }
        if loss != consent.accept_intentional_loss {
            return Err(Error::LossConsent);
        }
        let result = Self {
            schema: "tig.retirement-decision.design-1",
            scope: "explicit-admitted-decision-not-execution-permission",
            reviewed: review.digest().map_err(Error::Review)?,
            approval,
            consent,
            execution_context: None,
        };
        result.to_json()?;
        Ok(result)
    }
    /// A saved context requirement, never execution permission. Old entry points
    /// must refuse it unless they restore and enforce this exact context.
    pub fn authorize_guarded(
        review: &Review,
        approval: Approval,
        consent: Consent,
        context: Digest,
    ) -> Result<Self> {
        if !review.is_routine_eviction() {
            return Err(Error::Scope);
        }
        let mut result = Self::authorize(review, approval, consent)?;
        result.schema = "tig.retirement-decision.design-2";
        result.execution_context = Some(context);
        Ok(result)
    }
    pub fn execution_context(&self) -> Option<&Digest> {
        self.execution_context.as_ref()
    }
    pub fn from_json(bytes: &[u8], review: &Review) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Record {
            schema: String,
            scope: String,
            reviewed: Digest,
            approval: Approval,
            consent: Consent,
            execution_context: Option<Digest>,
        }
        let r: Record =
            serde_json::from_value(identity::parse_json(bytes).map_err(|_| Error::Encoding)?)
                .map_err(|_| Error::Encoding)?;
        if !matches!(
            (r.schema.as_str(), r.execution_context.as_ref()),
            ("tig.retirement-decision.design-1", None)
                | ("tig.retirement-decision.design-2", Some(_))
        ) || r.scope != "explicit-admitted-decision-not-execution-permission"
            || r.reviewed != review.digest().map_err(Error::Review)?
        {
            return Err(Error::Scope);
        }
        let result = match r.execution_context {
            Some(context) => Self::authorize_guarded(review, r.approval, r.consent, context)?,
            None => Self::authorize(review, r.approval, r.consent)?,
        };
        if result.to_json()? != bytes {
            return Err(Error::Encoding);
        }
        Ok(result)
    }
    pub fn approval(&self) -> &Approval {
        &self.approval
    }
    pub fn consent(&self) -> &Consent {
        &self.consent
    }
    pub fn reviewed(&self) -> &Digest {
        &self.reviewed
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        identity::canonical_json(&serde_json::to_value(self).map_err(|_| Error::Encoding)?)
            .map_err(|_| Error::Encoding)
    }
    pub fn digest(&self) -> Result<Digest> {
        Ok(Digest::blob(&self.to_json()?))
    }
}
