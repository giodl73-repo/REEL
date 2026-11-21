//! Explicit declared-roster inspection. No discovery, enrollment or payload IO.
use super::{invalid, retirement::MetadataBinding};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::PathBuf,
};
use tig_core::{Digest, Object};
use tig_store::{
    Key,
    metadata::MetadataReader,
    source_claims::{
        Checkpoint, Selection,
        protection::{Enrollment, Index},
    },
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedActor {
    pub revision: u64,
    pub selection: Selection,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub root: PathBuf,
    pub index: Digest,
    pub minimum: Checkpoint,
    pub metadata: MetadataBinding,
    pub enrollment: Enrollment,
    pub actors: BTreeMap<String, ExpectedActor>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub authorities: Vec<Authority>,
    /// Known participants whose admitted projection/coordination is unresolved.
    /// Empty is caller-declared scope, never evidence of universal coverage.
    pub unresolved: Vec<Key>,
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Unavailable,
    EnrollmentMismatch,
    Pending,
    Cancelled,
    EffectFencesMissing,
    ActorNotReady,
    ActorMismatch,
    UnresolvedParticipant,
}
#[derive(Serialize)]
pub struct Issue {
    pub authority: Option<usize>,
    pub actor: Option<Key>,
    pub reason: Reason,
}
#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub request: Object,
    pub checkpoints: Vec<Option<Checkpoint>>,
    pub declared_enrollment_matches: bool,
    pub issues: Vec<Issue>,
    pub complete_claim_coverage: bool,
    pub cutover_authorized: bool,
    pub removal_authorized: bool,
}

fn validate(request: &Request) -> io::Result<()> {
    if request.schema != "tig.annex-enrollment-inspection.experimental-1"
        || !(1..=8).contains(&request.authorities.len())
        || request.unresolved.len() > 128
        || request
            .unresolved
            .windows(2)
            .any(|p| p[0].as_str() >= p[1].as_str())
    {
        return Err(invalid());
    }
    let mut indexes = BTreeSet::new();
    let mut stores = BTreeSet::new();
    for a in &request.authorities {
        a.enrollment.validate().map_err(|_| invalid())?;
        if !a.root.is_absolute()
            || !a.metadata.root.is_absolute()
            || !indexes.insert(&a.index)
            || a.enrollment.stores.iter().any(|s| !stores.insert(s))
            || a.actors.keys().map(String::as_str).collect::<Vec<_>>()
                != a.enrollment
                    .actors
                    .iter()
                    .map(|p| p.id.as_str())
                    .collect::<Vec<_>>()
            || a.actors
                .values()
                .any(|p| p.revision == 0 || !(1..=65_536).contains(&p.selection.root.bytes))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Compare current observed control snapshots with exact caller expectations.
/// No pool commands, actor-root traversal, journals writes or approval grants.
/// Output contains private actor identifiers; it excludes physical root paths.
pub fn inspect(request: &Request, input: Object) -> io::Result<Report> {
    validate(request)?;
    let mut issues = Vec::new();
    let mut checkpoints = Vec::new();
    for (number, expected) in request.authorities.iter().enumerate() {
        let observed = MetadataReader::open(&expected.metadata.root, &expected.metadata.minimum)
            .map_err(|_| invalid())
            .and_then(|nodes| {
                Index::observe(&expected.root, &expected.minimum, &nodes).map_err(|_| invalid())
            });
        let mut issue = |actor, reason| {
            issues.push(Issue {
                authority: Some(number),
                actor,
                reason,
            })
        };
        let Ok(observed) = observed else {
            checkpoints.push(None);
            issue(None, Reason::Unavailable);
            continue;
        };
        checkpoints.push(Some(observed.checkpoint));
        if observed.index != expected.index || observed.enrollment != expected.enrollment {
            issue(None, Reason::EnrollmentMismatch);
            continue;
        }
        if observed.pending {
            issue(None, Reason::Pending);
        }
        if observed.cancelled {
            issue(None, Reason::Cancelled);
        }
        if !observed.effect_fences {
            issue(None, Reason::EffectFencesMissing);
        }
        for actor in &expected.enrollment.actors {
            let Some(state) = observed.actors.get(actor.id.as_str()) else {
                issue(Some(actor.id.clone()), Reason::ActorMismatch);
                continue;
            };
            let wanted = &expected.actors[actor.id.as_str()];
            if !state.ready {
                issue(Some(actor.id.clone()), Reason::ActorNotReady);
            }
            if state.revision != wanted.revision || state.selection != wanted.selection {
                issue(Some(actor.id.clone()), Reason::ActorMismatch);
            }
        }
    }
    for actor in &request.unresolved {
        issues.push(Issue {
            authority: None,
            actor: Some(actor.clone()),
            reason: Reason::UnresolvedParticipant,
        });
    }
    Ok(Report {
        schema: "tig.annex-enrollment-observation.experimental-1",
        request: input,
        checkpoints,
        declared_enrollment_matches: issues.is_empty(),
        issues,
        complete_claim_coverage: false,
        cutover_authorized: false,
        removal_authorized: false,
    })
}
