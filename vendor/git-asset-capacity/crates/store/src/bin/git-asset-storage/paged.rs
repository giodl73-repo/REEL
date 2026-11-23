//! Explicit, private-root operator over the existing paged retirement controller.
use super::bulk::{Nodes, Source, Store};
use super::{Result, document, raw, report};
use git_asset_core::{Digest, Object};
use git_asset_store::{
    Key,
    clock::{Enrollment as ClockEnrollment, LocalClock},
    final_retirement::{
        bulk, claims,
        paged::{self, control},
    },
    ledger::{CapacityLedger, Checkpoint},
    metadata::{MetadataReader, MetadataStore},
    source_claims::{admission::Namespace, age},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
#[path = "paged/execute.rs"]
mod execute;
#[path = "paged/followup.rs"]
mod followup;
const SCHEMA: &str = "tig.paged-operator-profile.design-1";
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Permissions {
    stage: bool,
    apply: bool,
    cancel: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clock {
    root: PathBuf,
    minimum: Checkpoint,
    enrollment: ClockEnrollment,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    root: PathBuf,
    minimum: git_asset_store::source_claims::Checkpoint,
    #[serde(default)]
    limits: age::Limits,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    enrollment: control::Enrollment,
    roster: claims::Roster,
    admitted_context: Digest,
    metadata: Nodes,
    namespaces: Vec<Source>,
    stores: Vec<Store>,
    witness: Store,
    clock: Option<Clock>,
    age_registry: Option<Registry>,
    claim_release: Option<Digest>,
    final_loss: Option<Digest>,
    #[serde(default)]
    permissions: Permissions,
    #[serde(default)]
    limits: paged::Limits,
    #[serde(default)]
    run_limits: control::RunLimits,
}
fn code(e: control::Error) -> &'static str {
    match e {
        control::Error::Invalid => "invalid_decision",
        control::Error::Authority => "wrong_authority",
        control::Error::Stale => "stale_basis",
        control::Error::Budget => "budget",
        control::Error::Recovery => "needs_recovery",
        control::Error::Selection(paged::Error::Budget) => "budget",
        control::Error::Provider(git_asset_store::final_retirement::provider::Error::Clock) => {
            "clock_window_held"
        }
        control::Error::Provider(_) => "provider_held",
        control::Error::Source(_) | control::Error::Admission(_) => "source_held",
        _ => "operation_failed",
    }
}
fn selection_code(e: paged::Error) -> &'static str {
    code(e.into())
}
fn bounded(value: Value) -> Result<Vec<u8>> {
    let bytes = report(value)?;
    if bytes.len() > 65536 {
        return Err("response_budget");
    }
    Ok(bytes)
}
impl Profile {
    fn load(path: &Path) -> Result<Self> {
        let p: Self = document(path)?;
        p.validate()
    }
    fn validate(self) -> Result<Self> {
        let p = self;
        p.enrollment.id().map_err(code)?;
        p.roster.id().map_err(|_| "invalid_roster")?;
        p.limits.validate().map_err(selection_code)?;
        if p.schema != SCHEMA
            || p.admitted_context != p.enrollment.context
            || p.roster.scope().map_err(|_| "invalid_roster")? != p.enrollment.selection.scope
            || p.witness.place != p.enrollment.witness
            || p.witness.minimum.identity() != &p.enrollment.capacity
            || p.metadata.minimum.identity() != &p.enrollment.selection.metadata
            || !p.metadata.root.is_absolute()
            || p.stores.len() > 4
            || p.namespaces.len() != p.roster.members.len()
            || p.run_limits.pages == 0
            || p.run_limits.pages > 64
            || p.run_limits.actions == 0
            || p.run_limits.actions > 32
        {
            return Err("invalid_profile");
        }
        let mut stores = BTreeSet::new();
        for s in p.stores.iter().chain(std::iter::once(&p.witness)) {
            if !s.root.is_absolute()
                || !s.ledger_root.is_absolute()
                || s.place.generation == 0
                || !stores.insert(s.place.instance.clone())
            {
                return Err("invalid_profile");
            }
        }
        let mut namespaces = BTreeSet::new();
        for n in &p.namespaces {
            if !n.root.is_absolute()
                || !namespaces.insert(n.minimum.identity().clone())
                || !p
                    .roster
                    .members
                    .iter()
                    .any(|m| &m.namespace == n.minimum.identity() && m.minimum == n.minimum)
            {
                return Err("invalid_profile");
            }
        }
        if let Some(r) = &p.age_registry {
            let policy = p.enrollment.age.as_ref().ok_or("invalid_registry")?;
            if !r.root.is_absolute()
                || r.minimum.identity() != &policy.enrollment.registry
                || namespaces.contains(r.minimum.identity())
            {
                return Err("invalid_registry");
            }
        }
        Ok(p)
    }
    fn copies(&self, copies: &[bulk::Copy]) -> Result<()> {
        for c in copies {
            let s = self
                .stores
                .iter()
                .find(|s| s.place.instance == c.store)
                .ok_or("unknown_store")?;
            if s.minimum.identity() != &c.capacity
                || c.publication.owner != self.enrollment.selection.owner
            {
                return Err("wrong_authority");
            }
        }
        Ok(())
    }
    fn clock(&self) -> Result<LocalClock> {
        let c = self.clock_config()?;
        let ledger = CapacityLedger::open(&c.root, &c.minimum).map_err(|_| "clock_unavailable")?;
        LocalClock::new(ledger, c.enrollment.clone()).map_err(|_| "invalid_clock")
    }
    fn clock_config(&self) -> Result<&Clock> {
        let c = self.clock.as_ref().ok_or("clock_required")?;
        if !c.root.is_absolute()
            || c.minimum.identity() != &c.enrollment.journal
            || c.enrollment.id().map_err(|_| "invalid_clock")? != self.enrollment.clock
        {
            return Err("invalid_clock");
        }
        Ok(c)
    }
    fn approve(&self, d: &control::Decision, cancel: bool) -> Result<()> {
        if !(if cancel {
            self.permissions.cancel
        } else {
            self.permissions.apply
        }) {
            return Err("execution_disabled");
        }
        let id = d.id().map_err(code)?;
        if d.enrollment_id() != &self.enrollment.id().map_err(code)? {
            return Err("wrong_authority");
        }
        if self.claim_release.as_ref() != Some(&id) || self.final_loss.as_ref() != Some(&id) {
            return Err("unapproved_decision");
        }
        if !cancel {
            self.clock_config()?;
        }
        if !cancel && self.enrollment.age.is_some() && self.age_registry.is_none() {
            return Err("registry_required");
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyRequest {
    schema: String,
    base: Option<Object>,
    copies: Vec<bulk::Copy>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanRequest {
    schema: String,
    operation: Key,
    selection: Object,
    copies: Object,
    window: git_asset_store::final_retirement::provider::Window,
}
fn stage(p: &Profile, operation: &str, input: &[u8]) -> Result<Vec<u8>> {
    if !p.permissions.stage {
        return Err("staging_disabled");
    }
    let value = git_asset_core::identity::parse_json(input).map_err(|_| "invalid_document")?;
    let copy: Option<CopyRequest> = if operation == "copies" {
        let r: CopyRequest =
            serde_json::from_value(value.clone()).map_err(|_| "invalid_document")?;
        if r.schema != "tig.paged-copy-request.design-1" {
            return Err("invalid_document");
        }
        p.copies(&r.copies)?;
        Some(r)
    } else {
        None
    };
    let plan: Option<PlanRequest> = if copy.is_none() {
        let r: PlanRequest = serde_json::from_value(value).map_err(|_| "invalid_document")?;
        if r.schema != "tig.paged-plan-request.design-1" {
            return Err("invalid_document");
        }
        Some(r)
    } else {
        None
    };
    let session = paged::Session::new(&p.limits).map_err(selection_code)?;
    let mut metadata = MetadataStore::open(&p.metadata.root, &p.metadata.minimum)
        .map_err(|_| "metadata_unavailable")?;
    if let Some(r) = copy {
        if let Some(base) = &r.base {
            let previous =
                control::read_copies(base, &mut session.nodes(&mut metadata), session.limits())
                    .map_err(code)?;
            p.copies(&previous)?;
        }
        let root = control::extend_copies(
            r.base.as_ref(),
            r.copies,
            &mut session.nodes(&mut metadata),
            session.limits(),
        )
        .map_err(code)?;
        let minimum = metadata.finish().map_err(|_| "metadata_finish")?;
        drop(metadata);
        let mut reader =
            MetadataReader::open(&p.metadata.root, &minimum).map_err(|_| "metadata_unavailable")?;
        let copies = control::read_copies(&root, &mut session.nodes(&mut reader), session.limits())
            .map_err(code)?;
        p.copies(&copies)?;
        return bounded(
            json!({"schema":"tig.paged-copy-response.design-1","copies":root,"count":copies.len(),"minimum":minimum,"authorizes_removal":false}),
        );
    }
    let r = plan.ok_or("invalid_document")?;
    let copies = control::read_copies(
        &r.copies,
        &mut session.nodes(&mut metadata),
        session.limits(),
    )
    .map_err(code)?;
    p.copies(&copies)?;
    let mut namespaces = p
        .namespaces
        .iter()
        .map(|s| Namespace::open(&s.root, &s.minimum).map_err(|_| "source_unavailable"))
        .collect::<Result<Vec<_>>>()?;
    let manifest = paged::build(
        &p.enrollment.selection,
        &r.selection,
        &p.roster,
        &mut namespaces,
        &mut session.nodes(&mut metadata),
        session.limits(),
    )
    .map_err(selection_code)?;
    // Read-only provider observations may hold a requested copy; no job/witness
    // or capacity ledger is opened during planning.
    let mut stores = Vec::new();
    for s in &p.stores {
        if let Ok(store) = git_asset_store::local::LocalStore::open(&s.root) {
            if git_asset_store::orchestration::Place::local(&store, s.place.generation)
                .map_err(|_| "wrong_store")?
                != s.place
            {
                return Err("wrong_store");
            }
            stores.push(store);
        }
    }
    let stores = stores.iter().collect::<Vec<_>>();
    let request = control::Request {
        operation: r.operation,
        manifest,
        copies: r.copies,
        window: r.window,
    };
    let decision = if p.enrollment.age.is_some() {
        let registry = p.age_registry.as_ref().ok_or("registry_required")?;
        let mut ns = Namespace::open(&registry.root, &registry.minimum)
            .map_err(|_| "registry_unavailable")?;
        let clock = p.clock()?;
        control::aged::plan(
            &p.enrollment,
            request,
            &p.roster,
            &mut namespaces,
            &mut session.nodes(&mut metadata),
            &stores,
            session.limits(),
            &mut control::aged::Context {
                registry: &mut ns,
                minimum: &registry.minimum,
                limits: &registry.limits,
            },
            &clock,
        )
        .map_err(code)?
    } else {
        control::plan(
            &p.enrollment,
            request,
            &p.roster,
            &mut namespaces,
            &mut session.nodes(&mut metadata),
            &stores,
            session.limits(),
        )
        .map_err(code)?
    };
    let minimum = metadata.finish().map_err(|_| "metadata_finish")?;
    drop(metadata);
    let mut reader =
        MetadataReader::open(&p.metadata.root, &minimum).map_err(|_| "metadata_unavailable")?;
    control::inspect(
        &p.enrollment,
        &decision,
        &mut session.nodes(&mut reader),
        session.limits(),
    )
    .map_err(code)?;
    bounded(
        json!({"schema":"tig.paged-plan-response.design-1","decision":decision,"decision_id":decision.id().map_err(code)?,"minimum":minimum,"authorizes_claim_release":false,"authorizes_removal":false}),
    )
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    schema: String,
    decision: Digest,
    kind: String,
    start: usize,
}
fn scope(
    p: &Profile,
    d: &control::Decision,
    kind: &str,
    cursor: Option<Cursor>,
) -> Result<Vec<u8>> {
    let id = d.id().map_err(code)?;
    let start = if let Some(c) = cursor {
        if c.schema != "tig.paged-scope-cursor.design-1" || c.decision != id || c.kind != kind {
            return Err("invalid_cursor");
        }
        c.start
    } else {
        0
    };
    let session = paged::Session::new(&p.limits).map_err(selection_code)?;
    let mut reader = MetadataReader::open(&p.metadata.root, &p.metadata.minimum)
        .map_err(|_| "metadata_unavailable")?;
    let inventory = control::inspect(
        &p.enrollment,
        d,
        &mut session.nodes(&mut reader),
        session.limits(),
    )
    .map_err(code)?;
    if kind == "inspect" {
        return bounded(
            json!({"schema":"tig.paged-inspection-response.design-1","decision_id":id,"decision":d,"subjects":inventory.subjects().len(),"actions":inventory.actions().len(),"historical_only":true,"authorizes_claim_release":false,"authorizes_removal":false}),
        );
    }
    let rows = if kind == "subjects" {
        inventory
            .subjects()
            .map(serde_json::to_value)
            .collect::<std::result::Result<Vec<_>, _>>()
    } else {
        inventory
            .actions()
            .map(|(store, action)| Ok(json!({"store":store,"plan":action})))
            .collect()
    }
    .map_err(|_| "response_budget")?;
    scope_page(id, kind, rows, start)
}
fn scope_page(id: Digest, kind: &str, rows: Vec<Value>, start: usize) -> Result<Vec<u8>> {
    if start > rows.len() {
        return Err("invalid_cursor");
    }
    let total = rows.len();
    let mut page = Vec::new();
    let mut bytes = 0;
    for row in rows.into_iter().skip(start).take(64) {
        let size = report(row.clone())?.len();
        if bytes + size > 49152 {
            break;
        }
        bytes += size;
        page.push(row);
    }
    if page.is_empty() && start < total {
        return Err("response_budget");
    }
    let end = start + page.len();
    let next = (end < total).then(|| Cursor {
        schema: "tig.paged-scope-cursor.design-1".into(),
        decision: id.clone(),
        kind: kind.into(),
        start: end,
    });
    bounded(
        json!({"schema":"tig.paged-scope-response.design-1","decision":id,"kind":kind,"start":start,"total":total,"rows":page,"next":next,"historical_only":true,"authorizes_claim_release":false,"authorizes_removal":false}),
    )
}

pub(super) fn run(args: &[String]) -> Result<Vec<u8>> {
    if args.first().is_some_and(|s| s == "followup") {
        return followup::run(&args[1..]);
    }
    if args.len() == 1 && args[0] == "describe" {
        return bounded(
            json!({"schema":"tig.paged-operator-interface.design-1","profile_schema":SCHEMA,"operations":["copies PROFILE REQUEST","plan PROFILE REQUEST","inspect PROFILE DECISION","subjects PROFILE DECISION [CURSOR]","actions PROFILE DECISION [CURSOR]","peak PROFILE DECISION","status PROFILE DECISION","apply PROFILE DECISION","resume PROFILE DECISION","cancel PROFILE DECISION"],"additional_interfaces":["paged followup describe"],"max_document_bytes":65536,"max_scope_rows":64,"permissions_default_enabled":false,"separate_claim_release_and_final_loss":true,"cancel_requires_clock":false,"initializes_stores":false}),
        );
    }
    if !(3..=4).contains(&args.len())
        || !matches!(
            args[0].as_str(),
            "copies"
                | "plan"
                | "inspect"
                | "subjects"
                | "actions"
                | "peak"
                | "status"
                | "apply"
                | "resume"
                | "cancel"
        )
        || (args.len() == 4 && !matches!(args[0].as_str(), "subjects" | "actions"))
    {
        return Err("usage");
    }
    let input = raw(Path::new(&args[2]))?;
    let decision = if matches!(args[0].as_str(), "copies" | "plan") {
        None
    } else {
        Some(control::Decision::from_json(&input).map_err(code)?)
    };
    let cursor = if args.len() == 4 {
        Some(document(Path::new(&args[3]))?)
    } else {
        None
    };
    let p = Profile::load(Path::new(&args[1]))?;
    match args[0].as_str() {
        "copies" | "plan" => stage(&p, &args[0], &input),
        "inspect" | "subjects" | "actions" => scope(
            &p,
            decision.as_ref().ok_or("invalid_decision")?,
            &args[0],
            cursor,
        ),
        _ => execute::run(&p, decision.as_ref().ok_or("invalid_decision")?, &args[0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_pages_cover_every_original_row_under_row_and_byte_limits() {
        for padding in [0, 10000] {
            let rows = (0..200)
                .map(|i| json!({"index":i,"fixture":"x".repeat(padding)}))
                .collect::<Vec<_>>();
            let id = Digest::blob(b"fixture-decision");
            let mut start = 0;
            let mut seen = Vec::new();
            loop {
                let raw = scope_page(id.clone(), "actions", rows.clone(), start).unwrap();
                assert!(raw.len() <= 65536);
                let page: Value = serde_json::from_slice(&raw).unwrap();
                let entries = page["rows"].as_array().unwrap();
                assert!(!entries.is_empty() && entries.len() <= 64);
                seen.extend(entries.iter().map(|r| r["index"].as_u64().unwrap()));
                if page["next"].is_null() {
                    break;
                }
                let cursor: Cursor = serde_json::from_value(page["next"].clone()).unwrap();
                assert_eq!(cursor.decision, id);
                assert_eq!(cursor.kind, "actions");
                assert!(cursor.start > start);
                start = cursor.start;
            }
            assert_eq!(seen, (0..200).collect::<Vec<_>>());
            assert!(scope_page(id, "actions", rows, 201).is_err());
        }
    }
}
