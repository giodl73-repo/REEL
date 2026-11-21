//! Bounded recorded fixture observations. No current custody or execution authority.
use super::*;
mod final_observation;
mod git_return;
mod paged_observation;
mod reacquisition;
#[cfg(feature = "upload")]
mod remote;
mod restoration;
use git_asset_core::{Digest, identity};
use git_asset_store::{
    existing::{ByteSource, Profile as ReaderProfile, ReadCache},
    placement,
};
use std::io::Read;
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub project: String,
    pub source: ReaderProfile,
    pub trace: Object,
    pub roles: Vec<Role>,
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.schema != "tig.catalogue-teaching-profile.design-1"
            || !token(&self.project)
            || self.trace.bytes == 0
            || self.trace.bytes > 65536
            || !(2..=4).contains(&self.roles.len())
            || self.roles.iter().any(|r| {
                !token(&r.id)
                    || r.label.is_empty()
                    || r.label.len() > 64
                    || r.label.chars().any(char::is_control)
            })
            || self
                .roles
                .iter()
                .map(|r| &r.id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.roles.len()
        {
            return Err("Invalid teaching enrollment".into());
        }
        Ok(())
    }
}
pub struct Request {
    profile: Profile,
    commits: BTreeSet<String>,
}
impl Catalogue {
    pub fn teaching_request(&self) -> Result<Request> {
        let profile = self
            .teaching
            .clone()
            .ok_or("No teaching fixture enrolled")?;
        let project = self
            .projects
            .get(&profile.project)
            .ok_or("Teaching project is not enrolled")?;
        let mut commits = project
            .refs
            .iter()
            .map(|(_, c)| c.clone())
            .collect::<BTreeSet<_>>();
        commits.insert(project.commit.clone());
        Ok(Request { profile, commits })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Trace {
    schema: String,
    completed: Vec<Entry>,
    pending: Vec<String>,
    fixture_native_scope: String,
    providers: Vec<ProviderRole>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderRole {
    instance: Digest,
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    lesson: String,
    report: Object,
    file: String,
    title: String,
    event: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    object: Object,
    verified_copies: Vec<String>,
    #[serde(default, deserialize_with = "unknown_roles")]
    unknown_copies: Option<Vec<String>>,
}
fn unknown_roles<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Option<Vec<String>>, D::Error> {
    Vec::<String>::deserialize(decoder).map(Some)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordedObserved {
    object: Object,
    provider: Digest,
    required: bool,
    state: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    plan: Digest,
    observed_basis: Digest,
    basis_current: bool,
    page_converged: bool,
    storage_converged: Option<bool>,
    stream_bytes: u64,
    effects: Vec<Effect>,
    observations: Vec<Value>,
    holds: Vec<String>,
    next: Option<placement::Cursor>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Effect {
    object: Object,
    provider: Digest,
    kind: String,
    logical_bytes: u64,
    file_allocation_before: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema: String,
    lesson: String,
    source_commit: String,
    source_checkpoint: git_asset_store::source_claims::Checkpoint,
    plan: placement::Plan,
    receipt: Receipt,
    observations: BTreeMap<String, Observation>,
    declared_fixture_domains: usize,
    physical_domain_coverage: String,
    independent_hardware_backup_proven: bool,
    #[serde(default, deserialize_with = "rejected_attempt")]
    rejection: Option<Rejected>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rejected {
    error: String,
    observed_basis: Digest,
}
fn rejected_attempt<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Option<Rejected>, D::Error> {
    Rejected::deserialize(decoder).map(Some)
}
fn raw_document(reader: &mut ReadCache, object: &Object, left: &mut u64) -> Result<Vec<u8>> {
    if object.bytes == 0 || object.bytes > 65536 || object.bytes > *left {
        return Err("Teaching read budget exceeded".into());
    }
    *left -= object.bytes;
    let mut source = reader
        .open_verified(object)
        .map_err(|_| "Teaching metadata unavailable or corrupt")?;
    let mut raw = Vec::new();
    source
        .by_ref()
        .take(65537)
        .read_to_end(&mut raw)
        .map_err(|_| "Teaching metadata read failed")?;
    if Object::from_bytes(&raw) != *object {
        return Err("Teaching metadata integrity failed".into());
    }
    Ok(raw)
}
fn document<T: serde::de::DeserializeOwned>(
    reader: &mut ReadCache,
    object: &Object,
    left: &mut u64,
) -> Result<T> {
    let raw = raw_document(reader, object, left)?;
    let value = parse_json(&raw).map_err(|_| "Invalid teaching JSON")?;
    if identity::canonical_json(&value).map_err(|_| "Invalid teaching JSON")? != raw {
        return Err("Noncanonical teaching JSON".into());
    }
    serde_json::from_value(value).map_err(|_| "Unsupported teaching metadata".into())
}
/// Metadata IO runs after the host releases its catalogue mutex.
pub fn execute(request: Request) -> Result<Value> {
    let p = request.profile;
    p.validate()?;
    let mut reader = ReadCache::new(p.source.clone()).map_err(|_| "Teaching reader unavailable")?;
    let mut left = 2 * 1024 * 1024;
    let trace: Trace = document(&mut reader, &p.trace, &mut left)?;
    if trace.schema != "tig.teaching-trace.design-2"
        || trace.completed.is_empty()
        || trace.completed.len() > 16
        || trace.pending.len() > 32
        || trace
            .pending
            .iter()
            .any(|s| s.len() > 160 || s.chars().any(char::is_control))
        || trace.fixture_native_scope.len() > 512
    {
        return Err("Invalid teaching trace".into());
    }
    if trace.providers.len() != p.roles.len()
        || trace
            .providers
            .iter()
            .zip(&p.roles)
            .any(|(a, b)| a.id != b.id)
        || trace
            .providers
            .iter()
            .map(|r| &r.instance)
            .collect::<BTreeSet<_>>()
            .len()
            != p.roles.len()
    {
        return Err("Teaching provider-role order differs".into());
    }
    let expected_providers = trace
        .providers
        .iter()
        .map(|r| r.instance.clone())
        .collect::<Vec<_>>();
    let roles = p
        .roles
        .iter()
        .map(|r| r.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut lessons = Vec::new();
    let mut names = BTreeSet::new();
    let mut objects = BTreeMap::<Digest, u64>::new();
    let mut identities = BTreeMap::<String, Object>::new();
    let mut providers = None;
    let mut coverage_v2 = false;
    let mut rejection_v3 = false;
    let mut final_v4 = false;
    let mut reacquisition_v5 = false;
    let mut joined = None;
    let mut returned_source = None;
    let mut returned_seed = None;
    #[cfg(feature = "upload")]
    let mut remote_token = None;
    #[cfg(feature = "upload")]
    let mut ordinary_source = None;
    for entry in trace.completed {
        if !token(&entry.lesson)
            || !names.insert(entry.lesson.clone())
            || entry.title.is_empty()
            || entry.title.len() > 120
            || entry.event.len() > 1024
            || entry
                .title
                .chars()
                .chain(entry.event.chars())
                .any(char::is_control)
            || entry.file.len() > 160
        {
            return Err("Invalid teaching lesson".into());
        }
        let value: Value = document(&mut reader, &entry.report, &mut left)?;
        if value["schema"] == "tig.teaching-remote-publication-observation.design-1" {
            #[cfg(not(feature = "upload"))]
            return Err("Remote teaching metadata requires upload support".into());
            #[cfg(feature = "upload")]
            {
                let (lesson, token) = remote::decode(
                    value,
                    remote::Context {
                        entry: &entry,
                        previous: lessons.last().ok_or("Missing exchange lesson")?,
                        binding: providers.as_ref().ok_or("Missing remote enrollment")?,
                        roles: &p.roles,
                        identities: &identities,
                        source: ordinary_source
                            .as_ref()
                            .ok_or("Missing exact exchange checkpoint")?,
                        commits: &request.commits,
                    },
                    remote_token.as_ref(),
                )?;
                remote_token = Some(token);
                lessons.push(lesson);
                coverage_v2 = true;
                continue;
            }
        }
        #[cfg(feature = "upload")]
        if remote_token.is_some() {
            return Err("Mixed remote teaching phases".into());
        }
        if value["schema"] == "tig.teaching-return-restore-observation.design-1" {
            let (lesson, token) = restoration::decode(
                value,
                restoration::Context {
                    entry: &entry,
                    lessons: &lessons,
                    binding: providers.as_ref().ok_or("Missing joined enrollment")?,
                    roles: &p.roles,
                    identities: &identities,
                    commits: &request.commits,
                    seed_attempts: returned_seed.ok_or("Missing returned seed counter")?,
                    source: returned_source
                        .as_ref()
                        .ok_or("Missing exact returned checkpoint")?,
                },
                joined.as_ref(),
            )?;
            joined = Some(token);
            lessons.push(lesson);
            coverage_v2 = true;
            continue;
        }
        if value["schema"] == "tig.teaching-git-return-observation.design-1" {
            let exact_seed = value["git_return"]["seed_attempts_after"]
                .as_u64()
                .ok_or("Invalid returned seed counter")?;
            let exact_source: git_asset_store::source_claims::Checkpoint =
                serde_json::from_value(value["source_checkpoint"].clone())
                    .map_err(|_| "Invalid returned checkpoint")?;
            lessons.push(git_return::decode(
                value,
                &entry,
                lessons.last().ok_or("Missing pre-return final lesson")?,
                providers.as_ref().ok_or("Missing pre-return enrollment")?,
                &p.roles,
                &identities,
                &request.commits,
            )?);
            returned_source = Some(exact_source);
            returned_seed = Some(exact_seed);
            final_v4 = true;
            coverage_v2 = true;
            continue;
        }
        if value["schema"] == "tig.teaching-reacquisition-observation.design-1" {
            lessons.push(reacquisition::decode(
                value,
                reacquisition::Context {
                    entry: &entry,
                    previous: lessons
                        .last()
                        .ok_or("Missing preceding acquisition lesson")?,
                    expected: &expected_providers,
                    roles: &p.roles,
                    identities: &identities,
                    commits: &request.commits,
                },
            )?);
            reacquisition_v5 = true;
            coverage_v2 = true;
            continue;
        }
        if value["schema"] == "tig.teaching-paged-final-observation.design-1" {
            lessons.push(paged_observation::decode(
                value,
                paged_observation::Context {
                    previous: lessons.last().ok_or("Missing preceding paged lesson")?,
                    entry: &entry,
                    expected: &expected_providers,
                    roles: &p.roles,
                    commits: &request.commits,
                    identities: &identities,
                    reader: &mut reader,
                    left: &mut left,
                },
            )?);
            final_v4 = true;
            coverage_v2 = true;
            continue;
        }
        if value["schema"] == "tig.teaching-final-observation.design-1" {
            lessons.push(final_observation::decode(
                value,
                &entry,
                &expected_providers,
                &p.roles,
                &request.commits,
                &identities,
                lessons.last().ok_or("Missing preceding final lesson")?,
            )?);
            final_v4 = true;
            coverage_v2 = true;
            continue;
        }
        let report: Report =
            serde_json::from_value(value).map_err(|_| "Unsupported teaching metadata")?;
        let id = report.plan.id().map_err(|_| "Invalid teaching plan")?;
        let binding = (
            report.plan.enrollment.clone(),
            report.plan.providers.clone(),
        );
        let v3 = report.schema == "tig.teaching-observation.design-3";
        let v2 = v3 || report.schema == "tig.teaching-observation.design-2";
        if v3 != report.rejection.is_some() {
            return Err("Rejected attempt version mismatch".into());
        }
        if let Some(rejected) = &report.rejection
            && (rejected.error != "stale"
                || rejected.observed_basis != report.receipt.observed_basis
                || rejected.observed_basis == report.plan.basis
                || report.receipt.basis_current
                || report.receipt.page_converged
                || report.receipt.storage_converged != Some(false)
                || report.receipt.stream_bytes != 0
                || !report.receipt.effects.is_empty()
                || report.receipt.next.is_some())
        {
            return Err("Inconsistent rejected attempt".into());
        }
        if (!v2 && report.schema != "tig.teaching-observation.design-1")
            || report.lesson != entry.lesson
            || report.independent_hardware_backup_proven
            || report.declared_fixture_domains != p.roles.len()
            || report.physical_domain_coverage.is_empty()
            || report.physical_domain_coverage.len() > 256
            || report.plan.providers.len() != p.roles.len()
            || report.plan.providers != expected_providers
            || report
                .plan
                .rows
                .iter()
                .map(|r| &r.object.sha256)
                .collect::<BTreeSet<_>>()
                .len()
                != report.plan.rows.len()
            || providers.as_ref().is_some_and(|old| old != &binding)
            || report.receipt.schema != "tig.placement-report.design-1"
            || report.receipt.plan != id
            || (!v3 && report.receipt.observed_basis != report.plan.basis)
            || report.receipt.effects.len() > 128
            || report.receipt.observations.len() > 128
            || report.receipt.holds.len() > 64
            || report.receipt.holds.iter().any(|s| {
                !matches!(
                    s.as_str(),
                    "invalid"
                        | "wrong_enrollment"
                        | "stale"
                        | "budget"
                        | "disabled"
                        | "busy"
                        | "unavailable"
                        | "integrity"
                        | "recovery"
                        | "protected"
                        | "retained"
                        | "missing_source"
                        | "reacquisition_required"
                        | "final_retirement"
                )
            })
            || report.receipt.page_converged && !report.receipt.basis_current
            || report.receipt.storage_converged == Some(true)
                && (!report.receipt.page_converged
                    || report.receipt.next.is_some()
                    || !report.receipt.holds.is_empty())
            || report.observations.len() > 32
            || report.observations.is_empty()
            || report
                .observations
                .values()
                .map(|o| &o.object.sha256)
                .collect::<BTreeSet<_>>()
                .len()
                != report.observations.len()
            || GitId::parse(&report.source_commit).is_err()
        {
            return Err("Inconsistent teaching report".into());
        }
        providers = Some(binding);
        coverage_v2 |= v2;
        rejection_v3 |= v3;
        let mut recorded = Vec::new();
        if v2 {
            let mut pairs = BTreeSet::new();
            for value in &report.receipt.observations {
                let observation: RecordedObserved = serde_json::from_value(value.clone())
                    .map_err(|_| "Invalid partial receipt observation")?;
                let column = report
                    .plan
                    .providers
                    .iter()
                    .position(|p| p == &observation.provider)
                    .ok_or("Unknown partial receipt provider")?;
                let row = report
                    .plan
                    .rows
                    .iter()
                    .find(|r| r.object == observation.object)
                    .ok_or("Unknown partial receipt object")?;
                let valid_state = observation.state.as_str().is_some_and(|s| {
                    matches!(
                        s,
                        "not_started"
                            | "untracked_present"
                            | "partial"
                            | "published"
                            | "retained"
                            | "released"
                            | "retirement_pending"
                            | "authorized_absence"
                            | "cancelled"
                    )
                }) || observation.state.as_object().is_some_and(|m| {
                    m.len() == 1
                        && m.get("held")
                            .and_then(Value::as_str)
                            .is_some_and(|s| report.receipt.holds.iter().any(|h| h == s))
                });
                if row.required.len() != p.roles.len()
                    || row.required[column] != observation.required
                    || !valid_state
                    || !pairs.insert((
                        observation.object.sha256.clone(),
                        observation.provider.clone(),
                    ))
                {
                    return Err("Inconsistent partial receipt observation".into());
                }
                recorded.push(observation);
            }
            if recorded.len() != report.plan.rows.len() * p.roles.len() {
                return Err("Incomplete partial receipt observations".into());
            }
        }
        let mut rows = Vec::new();
        for (name, observation) in report.observations {
            let object = &observation.object;
            if v2 != observation.unknown_copies.is_some() {
                return Err("Unknown teaching coverage version".into());
            }
            let unknown = observation.unknown_copies.unwrap_or_default();
            if unknown.windows(2).any(|p| p[0] >= p[1])
                || unknown.iter().any(|role| {
                    !roles.contains(role.as_str()) || observation.verified_copies.contains(role)
                })
            {
                return Err("Conflicting unknown teaching coverage".into());
            }
            if !unknown.is_empty()
                && (report.receipt.page_converged || report.receipt.storage_converged == Some(true))
            {
                return Err("Unknown coverage cannot converge".into());
            }
            for role in &unknown {
                let column = p
                    .roles
                    .iter()
                    .position(|r| &r.id == role)
                    .ok_or("Unknown teaching role")?;
                if !recorded.iter().any(|r| {
                    r.object == *object
                        && r.provider == report.plan.providers[column]
                        && r.state.get("held").and_then(Value::as_str).is_some()
                }) {
                    return Err("Unknown coverage lacks held provider evidence".into());
                }
            }
            for r in recorded
                .iter()
                .filter(|r| r.object == *object && r.state.get("held").is_some())
            {
                let column = report
                    .plan
                    .providers
                    .iter()
                    .position(|p| p == &r.provider)
                    .ok_or("Unknown partial provider")?;
                let role = &p.roles[column].id;
                if !observation.verified_copies.contains(role) && !unknown.contains(role) {
                    return Err("Held provider coverage omitted".into());
                }
            }
            if !token(&name)
                || observation
                    .verified_copies
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
                || observation
                    .verified_copies
                    .iter()
                    .any(|r| !roles.contains(r.as_str()))
                || objects
                    .get(&object.sha256)
                    .is_some_and(|bytes| *bytes != object.bytes)
                || identities.get(&name).is_some_and(|old| old != object)
            {
                return Err("Conflicting teaching object or role".into());
            }
            objects.insert(object.sha256.clone(), object.bytes);
            identities.insert(name.clone(), object.clone());
            let planned = report.plan.rows.iter().find(|r| r.object == *object);
            if planned.is_some_and(|r| r.required.len() != p.roles.len()) {
                return Err("Invalid teaching role order".into());
            }
            let desired = planned.map(|r| {
                r.required
                    .iter()
                    .zip(&p.roles)
                    .filter(|(yes, _)| **yes)
                    .map(|(_, role)| role.id.clone())
                    .collect::<Vec<_>>()
            });
            rows.push(json!({"name":name,"object":object,"desired":desired,"observed":observation.verified_copies,"unknown":unknown}));
        }
        if identities.len() > 32
            || report
                .plan
                .rows
                .iter()
                .any(|row| !rows.iter().any(|v| v["object"] == json!(row.object)))
        {
            return Err("Incomplete teaching object scope".into());
        }
        let mut effects = Vec::new();
        for effect in report.receipt.effects {
            let provider = report
                .plan
                .providers
                .iter()
                .position(|p| p == &effect.provider)
                .ok_or("Unknown teaching effect provider")?;
            if !matches!(
                effect.kind.as_str(),
                "published"
                    | "retained"
                    | "handoff"
                    | "retired"
                    | "reacquired"
                    | "cancelled"
                    | "recovered"
            ) || !rows.iter().any(|v| v["object"] == json!(effect.object))
            {
                return Err("Unknown teaching effect".into());
            }
            effects.push(json!({"object":effect.object,"role":p.roles[provider].id,"kind":effect.kind,"logical_bytes":effect.logical_bytes,"file_allocation_before":effect.file_allocation_before}));
        }
        lessons.push(json!({"id":entry.lesson,"title":entry.title,"event":entry.event,"report":entry.report,"plan":id,
            "source_commit":report.source_commit,"source_available":request.commits.contains(&report.source_commit),
            "source_sequence":report.source_checkpoint.sequence(),"objects":rows,"stream_bytes":report.receipt.stream_bytes,
            "basis_current":report.receipt.basis_current,"converged":report.receipt.page_converged,"storage_converged":report.receipt.storage_converged,
            "remaining_page":report.receipt.next.is_some(),"holds":report.receipt.holds,"effects":effects}));
        #[cfg(feature = "upload")]
        {
            ordinary_source = Some(report.source_checkpoint.clone());
        }
        if let Some(rejected) = report.rejection {
            let lesson = lessons.last_mut().ok_or("Missing rejected lesson")?;
            lesson["rejection"] = json!({"error":rejected.error,"planning_basis":report.plan.basis,"observed_basis":rejected.observed_basis});
        }
    }
    if !coverage_v2 {
        for lesson in &mut lessons {
            for row in lesson["objects"]
                .as_array_mut()
                .ok_or("Invalid lesson rows")?
            {
                row.as_object_mut()
                    .ok_or("Invalid lesson row")?
                    .remove("unknown");
            }
        }
    }
    if joined.as_ref().is_some_and(|t| !t.finished) {
        return Err("Incomplete joined restoration book".into());
    }
    #[cfg(feature = "upload")]
    let remote_v7 = {
        if remote_token.as_ref().is_some_and(|t| !t.finished) {
            return Err("Incomplete remote teaching book".into());
        }
        remote_token.is_some()
    };
    #[cfg(not(feature = "upload"))]
    let remote_v7 = false;
    let response = json!({"schema":if remote_v7 {"tig.catalogue-teaching.design-7"}else if joined.is_some(){"tig.catalogue-teaching.design-6"}else if reacquisition_v5 {"tig.catalogue-teaching.design-5"}else if final_v4 {"tig.catalogue-teaching.design-4"}else if rejection_v3 {"tig.catalogue-teaching.design-3"}else if coverage_v2 {"tig.catalogue-teaching.design-2"}else{"tig.catalogue-teaching.design-1"},"basis":p.trace,"project":p.project,"roles":p.roles,
        "lessons":lessons,"pending":trace.pending,"native_scope":trace.fixture_native_scope,
        "mode":"recorded-rehearsal","synthetic_domains":true,"independent_hardware_backup_proven":false,"authorizes_storage":false});
    if serde_json::to_vec(&response)
        .map_err(|_| "Teaching response refused")?
        .len()
        > 65536
    {
        return Err("Teaching response budget exceeded".into());
    }
    Ok(response)
}
