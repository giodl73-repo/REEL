//! Resolve current metadata and replay retained claims; no media IO or execution.
use super::produced::{self, Admission};
use serde_json::Value;
use tig_assets::{
    graph::{
        Reference,
        recipe::{
            Plan,
            assessment::{self, Key, Observation, State},
            produced::{BoundSubject, Claim, Selector},
        },
    },
    native::MAX_RECORD_BYTES,
    resolve::AssetResolver,
};
use tig_core::{Object, tree::NodeStore};

/// Literal metadata object keys, never physical paths or executable hooks.
pub struct ClaimLocation(Vec<String>);
impl ClaimLocation {
    pub fn new(segments: Vec<String>) -> super::Result<Self> {
        if !(1..=8).contains(&segments.len())
            || segments
                .iter()
                .any(|s| s.is_empty() || s.len() > 128 || s.chars().any(char::is_control))
        {
            return Err(super::Error::Invalid);
        }
        Ok(Self(segments))
    }
    fn read(&self, metadata: &serde_json::Map<String, Value>) -> Result<Option<Object>, Issue> {
        let Some(mut value) = metadata.get(&self.0[0]) else {
            return Ok(None);
        };
        for key in &self.0[1..] {
            value = value
                .as_object()
                .and_then(|m| m.get(key))
                .ok_or(Issue::ClaimPin)?;
        }
        let pin: Object = serde_json::from_value(value.clone()).map_err(|_| Issue::ClaimPin)?;
        if !(1..=MAX_RECORD_BYTES as u64).contains(&pin.bytes) {
            return Err(Issue::ClaimPin);
        }
        Ok(Some(pin))
    }
}

pub struct Request<'a> {
    pub key: &'a Key,
    pub desired: &'a Plan,
    pub definition: &'a Reference,
    pub claim_location: &'a ClaimLocation,
    /// External admission for the original claim, independent of desired producer.
    pub original: &'a Admission<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Issue {
    InvalidRequest,
    CurrentUnavailable,
    CurrentCheckpoint,
    CurrentDefinition,
    ClaimPin,
    ClaimUnavailable,
    ClaimIntegrity,
    ClaimReplay,
    ClaimSubject,
}

/// Private runtime observation, not a durable capability or complete project view.
pub struct OutputAssessment {
    pub state: State,
    pub issue: Option<Issue>,
    pub record: Option<Reference>,
    pub claim: Option<Object>,
}
impl std::fmt::Debug for OutputAssessment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputAssessment")
            .field("state", &self.state)
            .field("issue", &self.issue)
            .field("record_present", &self.record.is_some())
            .field("claim_present", &self.claim.is_some())
            .finish_non_exhaustive()
    }
}

pub fn assess(
    request: &Request<'_>,
    resolver: &(impl AssetResolver + ?Sized),
    store: &mut impl NodeStore,
) -> OutputAssessment {
    let mut result = OutputAssessment {
        state: State::Blocked,
        issue: None,
        record: None,
        claim: None,
    };
    match evaluate(request, resolver, store, &mut result) {
        Ok(state) => result.state = state,
        Err(issue) => result.issue = Some(issue),
    }
    result
}

fn evaluate(
    request: &Request<'_>,
    resolver: &(impl AssetResolver + ?Sized),
    store: &mut impl NodeStore,
    result: &mut OutputAssessment,
) -> Result<State, Issue> {
    let key = request.key;
    // Bound clones before validation; the caller's wire parser is not assumed.
    if key.asset_id.len() > 256
        || matches!(&key.selector, Selector::Stream { slot } if slot.len() > 128)
    {
        return Err(Issue::InvalidRequest);
    }
    let probe = assessment::Request {
        key: key.clone(),
        desired: None,
        current: Observation::Absent,
        claim: Observation::Absent,
    };
    assessment::assess(request.desired.checkpoint(), &[probe], 1)
        .map_err(|_| Issue::InvalidRequest)?;
    let current = resolver
        .resolve_id(&key.asset_id)
        .map_err(|_| Issue::CurrentUnavailable)?;
    result.record = Some(current.reference().clone());
    if current.checkpoint() != request.desired.checkpoint()
        || current.reference().id != key.asset_id
    {
        return Err(Issue::CurrentCheckpoint);
    }
    let subject = BoundSubject::resolve(&current, request.definition, key.selector.clone())
        .map_err(|_| Issue::CurrentDefinition)?;
    let pin = request.claim_location.read(current.metadata())?;
    result.claim = pin.clone();
    let mut entry = assessment::Request {
        key: key.clone(),
        desired: Some(request.desired),
        current: subject
            .as_ref()
            .map_or(Observation::Absent, Observation::Present),
        claim: Observation::Absent,
    };
    let Some(pin) = pin else {
        return Ok(
            assessment::assess(request.desired.checkpoint(), &[entry], 1)
                .map_err(|_| Issue::InvalidRequest)?
                .entries()[0]
                .state,
        );
    };
    let raw = store
        .get(&pin)
        .map_err(|_| Issue::ClaimUnavailable)?
        .ok_or(Issue::ClaimUnavailable)?;
    let claim = Claim::parse(&pin, &raw).map_err(|_| Issue::ClaimIntegrity)?;
    let (_, replayed) =
        produced::replay(&claim, request.original, store).map_err(|_| Issue::ClaimReplay)?;
    entry.claim = Observation::Present(replayed.checked());
    let report = assessment::assess(request.desired.checkpoint(), &[entry], 1)
        .map_err(|_| Issue::InvalidRequest)?;
    let state = report.entries()[0].state;
    if state == State::Blocked {
        return Err(Issue::ClaimSubject);
    }
    Ok(state)
}
