//! Bounded desired-placement pages over existing publication/retention/removal.
//! No discovery, independent journal, remote custody or final-copy deletion.
mod execute;
#[cfg(test)]
mod tests;
use crate::{
    Key,
    ledger::{CapacityLedger, JournalRef},
    local::LocalStore,
    orchestration::Place,
    protection::Limits,
    source_claims::{Checkpoint, admission::Namespace},
    store_protection::Member,
};
pub use execute::{Effect, EffectKind, Observation, Observed, Report, publication};
use git_asset_core::{Digest, Object, identity, tree::NodeStore};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    Invalid,
    WrongEnrollment,
    Stale,
    Budget,
    Disabled,
    Busy,
    Unavailable,
    Integrity,
    Recovery,
    Protected,
    Retained,
    MissingSource,
    ReacquisitionRequired,
    FinalRetirement,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    let b = identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    if b.len() > 65536 {
        return Err(Error::Budget);
    }
    Ok(b)
}
fn digest(v: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(v)?))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Cache,
    Retained,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub namespace: Digest,
    pub place: Key,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub place: Place,
    pub role: Role,
    pub failure_domain: Key,
    pub capacity: Digest,
    pub job: JournalRef,
    pub assignments: Vec<Mapping>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub namespace: Digest,
    pub policy: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub objects: usize,
    pub max_object_bytes: u64,
    pub max_stream_bytes: u64,
    pub max_peak_bytes: u64,
    pub limits: Limits,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub owner: Key,
    pub authority: Digest,
    pub seed: Digest,
    pub providers: Vec<Provider>,
    pub namespaces: Vec<Authority>,
    pub minimum_domains: usize,
    pub budget: Budget,
}
impl Config {
    pub fn id(&self) -> Result<Digest> {
        if !matches!(
            self.schema.as_str(),
            "tig.placement-enrollment.design-1" | "tig.placement-enrollment.design-2"
        ) || self.providers.len() < 2
            || self.providers.len() > 4
            || self.namespaces.is_empty()
            || self.namespaces.len() > 16
            || self.minimum_domains == 0
            || self.budget.objects == 0
            || self.budget.objects > 32
            || self.budget.max_object_bytes == 0
            || self.budget.max_object_bytes > 8 * 1024 * 1024
            || self.budget.max_stream_bytes == 0
            || self.budget.max_stream_bytes > 1024 * 1024 * 1024
            || self.budget.max_peak_bytes == 0
            || self.budget.max_peak_bytes > 1024 * 1024 * 1024
        {
            return Err(Error::Budget);
        }
        self.budget.limits.validate().map_err(|_| Error::Budget)?;
        let mut ids = BTreeSet::new();
        let mut maps = BTreeSet::new();
        for provider in &self.providers {
            if provider.place.generation == 0
                || !ids.insert(provider.place.instance.clone())
                || provider.assignments.len() > 256
            {
                return Err(Error::WrongEnrollment);
            }
            for m in &provider.assignments {
                if !maps.insert((m.namespace.clone(), m.place.as_str().to_owned())) {
                    return Err(Error::WrongEnrollment);
                }
            }
        }
        let mut namespaces = BTreeSet::new();
        for ns in &self.namespaces {
            if !namespaces.insert(ns.namespace.clone()) {
                return Err(Error::WrongEnrollment);
            }
        }
        if self
            .providers
            .iter()
            .filter(|p| p.role == Role::Retained)
            .count()
            < self.minimum_domains
            || !self.providers.iter().any(|p| p.role == Role::Cache)
        {
            return Err(Error::Invalid);
        }
        let mut providers = self.providers.clone();
        for provider in &mut providers {
            provider.assignments.sort_by(|a, b| {
                a.namespace
                    .cmp(&b.namespace)
                    .then_with(|| a.place.as_str().cmp(b.place.as_str()))
            });
        }
        providers.sort_by(|a, b| a.place.instance.cmp(&b.place.instance));
        let mut namespaces = self.namespaces.clone();
        namespaces.sort_by(|a, b| a.namespace.cmp(&b.namespace));
        // Page/run budgets must not create another physical placement identity.
        digest(&(
            &self.schema,
            &self.owner,
            &self.authority,
            &self.seed,
            providers,
            namespaces,
            self.minimum_domains,
        ))
    }
}
pub struct Local<'a> {
    pub store: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub job: &'a JournalRef,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub basis: Digest,
    pub after: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub object: Object,
    pub required: Vec<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    pub enrollment: Digest,
    pub providers: Vec<Digest>,
    pub budget: Budget,
    pub basis: Digest,
    pub after: Option<Digest>,
    pub rows: Vec<Row>,
    pub next: Option<Cursor>,
}
impl Plan {
    pub fn id(&self) -> Result<Digest> {
        if self.schema != "tig.placement-plan.design-1" || self.rows.len() > 32 {
            return Err(Error::Invalid);
        }
        digest(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(b: &[u8]) -> Result<Self> {
        if b.len() > 65536 {
            return Err(Error::Budget);
        }
        let p: Self = serde_json::from_value(identity::parse_json(b).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
        if p.to_json()? != b {
            return Err(Error::Invalid);
        }
        Ok(p)
    }
}
struct Desired {
    basis: Digest,
    entries: BTreeMap<Digest, Row>,
}
pub struct Execution<'a, 'b, S, N> {
    pub config: &'a Config,
    pub namespaces: &'a mut [Namespace],
    pub nodes: &'a N,
    pub places: &'a mut [Local<'b>],
    pub seed: &'a mut S,
    pub seed_identity: &'a Digest,
    pub allow_copy: bool,
    pub allow_eviction: bool,
    pub allow_reacquisition: bool,
    pub allow_cancellation: bool,
}
impl<S, N: NodeStore> Execution<'_, '_, S, N> {
    fn check(&self) -> Result<()> {
        self.config.id()?;
        if self.seed_identity != &self.config.seed
            || self.places.len() != self.config.providers.len()
            || self.namespaces.len() != self.config.namespaces.len()
        {
            return Err(Error::WrongEnrollment);
        }
        let mut actual = BTreeSet::new();
        for ns in self.namespaces.iter() {
            if !actual.insert(ns.identity().clone()) {
                return Err(Error::WrongEnrollment);
            }
            let pin = self
                .config
                .namespaces
                .iter()
                .find(|p| &p.namespace == ns.identity())
                .ok_or(Error::WrongEnrollment)?;
            if digest(ns.policy())? != pin.policy {
                return Err(Error::WrongEnrollment);
            }
        }
        for (runtime, pin) in self.places.iter().zip(&self.config.providers) {
            if runtime.store.identity() != &pin.place.instance
                || runtime.ledger.identity() != &pin.capacity
                || runtime.job != &pin.job
            {
                return Err(Error::WrongEnrollment);
            }
            if pin.role == Role::Cache && !runtime.store.supports_retirement() {
                return Err(Error::WrongEnrollment);
            }
            if let Some(binding) = runtime.store.protection() {
                let roster = self.config.schema == "tig.placement-enrollment.design-2";
                if (binding.history_scope() == crate::store_protection::HistoryScope::Roster)
                    != roster
                    || roster && binding.members().len() != self.namespaces.len()
                {
                    return Err(Error::WrongEnrollment);
                }
                let mut expected = BTreeMap::<Digest, Vec<Key>>::new();
                for m in &pin.assignments {
                    expected
                        .entry(m.namespace.clone())
                        .or_default()
                        .push(m.place.clone());
                }
                for keys in expected.values_mut() {
                    keys.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                }
                let members = expected
                    .into_iter()
                    .map(|(namespace, places)| Member { namespace, places })
                    .collect::<Vec<_>>();
                if members != binding.members() {
                    return Err(Error::WrongEnrollment);
                }
            }
        }
        for ns in self.namespaces.iter() {
            let keys = ns
                .policy()
                .classes
                .values()
                .flat_map(|p| p.current.iter().chain(&p.pending).chain(&p.predecessor))
                .map(|k| k.as_str())
                .collect::<BTreeSet<_>>();
            let assigned = self
                .config
                .providers
                .iter()
                .flat_map(|p| &p.assignments)
                .filter(|m| &m.namespace == ns.identity())
                .map(|m| m.place.as_str())
                .collect::<BTreeSet<_>>();
            if keys != assigned {
                return Err(Error::WrongEnrollment);
            }
        }
        if self
            .config
            .providers
            .iter()
            .flat_map(|p| &p.assignments)
            .any(|m| !actual.contains(&m.namespace))
        {
            return Err(Error::WrongEnrollment);
        }
        Ok(())
    }
    fn desired(&mut self) -> Result<Desired> {
        self.check()?;
        let mut work = crate::source_claims::admission::Work::new(
            self.nodes,
            &self.config.budget.limits,
            std::time::Instant::now(),
        )
        .map_err(|_| Error::Budget)?;
        let mut observations = Vec::<(Digest, Checkpoint)>::new();
        let mut entries = BTreeMap::<Digest, Row>::new();
        let observed = if self.config.schema == "tig.placement-enrollment.design-2" {
            crate::source_claims::admission::roster::requirements(self.namespaces, &mut work)
        } else {
            self.namespaces
                .iter_mut()
                .map(|ns| ns.requirements_work(&mut work))
                .collect()
        }
        .map_err(|e| match e {
            crate::source_claims::admission::Error::Budget => Error::Budget,
            _ => Error::Recovery,
        })?;
        for observed in observed {
            observations.push((observed.namespace.clone(), observed.checkpoint));
            for entry in observed.entries {
                let row = entries
                    .entry(entry.object.sha256.clone())
                    .or_insert_with(|| Row {
                        object: entry.object.clone(),
                        required: self
                            .config
                            .providers
                            .iter()
                            .map(|p| p.role == Role::Retained)
                            .collect(),
                    });
                if row.object != entry.object {
                    return Err(Error::Integrity);
                }
                for key in entry.places {
                    let index = self
                        .config
                        .providers
                        .iter()
                        .position(|p| {
                            p.assignments
                                .iter()
                                .any(|m| m.namespace == observed.namespace && m.place == key)
                        })
                        .ok_or(Error::WrongEnrollment)?;
                    row.required[index] = true;
                }
            }
        }
        observations.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Desired {
            basis: digest(&(
                "tig.placement-basis.design-1",
                self.config.id()?,
                observations,
            ))?,
            entries,
        })
    }
    pub fn plan(&mut self, cursor: Option<&Cursor>) -> Result<Plan> {
        let desired = self.desired()?;
        self.page(&desired, cursor)
    }
    fn page(&self, desired: &Desired, cursor: Option<&Cursor>) -> Result<Plan> {
        if cursor.is_some_and(|c| c.basis != desired.basis) {
            return Err(Error::Stale);
        }
        let after = cursor.map(|c| c.after.clone());
        let mut candidates = desired
            .entries
            .values()
            .filter(|r| after.as_ref().is_none_or(|a| r.object.sha256 > *a))
            .cloned();
        let rows = candidates
            .by_ref()
            .take(self.config.budget.objects)
            .collect::<Vec<_>>();
        if rows
            .iter()
            .any(|r| r.object.bytes > self.config.budget.max_object_bytes)
        {
            return Err(Error::Budget);
        }
        let next = if candidates.next().is_some() {
            rows.last().map(|r| Cursor {
                basis: desired.basis.clone(),
                after: r.object.sha256.clone(),
            })
        } else {
            None
        };
        let p = Plan {
            schema: "tig.placement-plan.design-1".into(),
            enrollment: self.config.id()?,
            providers: self
                .config
                .providers
                .iter()
                .map(|p| p.place.instance.clone())
                .collect(),
            budget: self.config.budget.clone(),
            basis: desired.basis.clone(),
            after,
            rows,
            next,
        };
        p.id()?;
        Ok(p)
    }
    fn admit(&mut self, p: &Plan) -> Result<()> {
        if p.enrollment != self.config.id()? {
            return Err(Error::WrongEnrollment);
        }
        let cursor = p.after.as_ref().map(|after| Cursor {
            basis: p.basis.clone(),
            after: after.clone(),
        });
        if self.plan(cursor.as_ref())?.to_json()? != p.to_json()? {
            return Err(Error::Stale);
        }
        Ok(())
    }
}
