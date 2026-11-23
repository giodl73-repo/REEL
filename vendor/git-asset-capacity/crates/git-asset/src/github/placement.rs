//! Bounded authenticated-source coordination over the existing local executor.
use super::*;
use crate::lifecycle::{self, CommittedSource};
use git_asset_store::{
    existing::ByteSource, metadata::MetadataStore, placement as local,
    source_claims::admission::Namespace,
};
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Permissions {
    pub selection: bool,
    pub copy: bool,
    pub eviction: bool,
    pub reacquisition: bool,
    pub cancellation: bool,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub pages: u32,
    pub objects: usize,
    pub stream_bytes: u64,
    pub millis: u64,
}
impl Budget {
    pub fn validate(&self) -> Result<()> {
        if self.pages == 0
            || self.pages > 16384
            || self.objects == 0
            || self.objects > 500000
            || self.stream_bytes == 0
            || self.stream_bytes > 1024 * 1024 * 1024 * 1024
            || self.millis == 0
            || self.millis > 600000
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    Admission(Error),
    Placement(local::Error),
}
#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub provenance: Provenance,
    pub selection: lifecycle::Status,
    pub pages: u32,
    pub objects: usize,
    pub stream_bytes: u64,
    pub effects: usize,
    pub basis: Option<Digest>,
    pub next: Option<local::Cursor>,
    pub holds: Vec<Hold>,
    pub storage_converged: Option<bool>,
}
impl Report {
    fn hold(&mut self, reason: Hold) {
        // Deduplicate on admission, so memory stays bounded by the static reason
        // vocabulary even across a full pass of held objects.
        let identity = serde_json::to_string(&reason).unwrap_or_default();
        if !self
            .holds
            .iter()
            .any(|h| serde_json::to_string(h).unwrap_or_default() == identity)
        {
            self.holds.push(reason);
        }
    }
}
pub struct Execution<'a, 'b, S> {
    pub trust: &'a Trust,
    pub request: &'a Verified,
    pub client: &'a http::Client,
    pub enrollment: &'a Enrollment,
    pub config: &'a local::Config,
    pub namespaces: &'a mut [Namespace],
    pub metadata: &'a mut MetadataStore,
    pub places: &'a mut [local::Local<'b>],
    pub seed: &'a mut S,
    pub seed_identity: &'a Digest,
    pub permissions: Permissions,
    pub budget: &'a Budget,
}
fn selection_error(error: lifecycle::Error) -> Error {
    match error {
        lifecycle::Error::Budget => Error::Budget,
        lifecycle::Error::Stale => Error::Stale,
        lifecycle::Error::Authority => Error::Trust,
        lifecycle::Error::Disabled => Error::Disabled,
        _ => Error::Unavailable,
    }
}
fn selected_now(
    namespaces: &mut [Namespace],
    enrollment: &Enrollment,
    plan: &lifecycle::Plan,
) -> Result<lifecycle::Status> {
    let namespace = namespaces
        .iter_mut()
        .find(|n| n.identity() == &enrollment.namespace)
        .ok_or(Error::Trust)?;
    let mut status =
        lifecycle::inspect_selection(enrollment, namespace, plan).map_err(selection_error)?;
    status.observed_ref_current = Some(true);
    Ok(status)
}
impl<S: ByteSource> Execution<'_, '_, S>
where
    S::Reader: 'static,
{
    pub fn run(
        &mut self,
        cursor: Option<&local::Cursor>,
        mut now: impl FnMut() -> Result<u64>,
    ) -> Result<Report> {
        self.budget.validate()?;
        if self.request.provenance.trust != self.trust.id()? {
            return Err(Error::Trust);
        }
        let selection_id = self.enrollment.id().map_err(selection_error)?;
        let placement_id = self.config.id().map_err(|_| Error::Trust)?;
        self.request.admit(now()?, &selection_id, &placement_id)?;
        if self.config.owner != self.enrollment.owner
            || self.config.authority != self.enrollment.authority
            || !self.config.namespaces.iter().any(|n| {
                n.namespace == self.enrollment.namespace && n.policy == self.enrollment.policy
            })
        {
            return Err(Error::Trust);
        }
        if !self.permissions.selection {
            return Err(Error::Disabled);
        }
        {
            let nodes = self
                .metadata
                .finished_reader()
                .map_err(|_| Error::Unavailable)?;
            local::Execution {
                config: self.config,
                namespaces: self.namespaces,
                nodes: &nodes,
                places: self.places,
                seed: self.seed,
                seed_identity: self.seed_identity,
                allow_copy: false,
                allow_eviction: false,
                allow_reacquisition: false,
                allow_cancellation: false,
            }
            .plan(None)
            .map_err(|_| Error::Trust)?;
        }
        let source = source::Source {
            client: self.client,
            trust: self.trust,
        };
        let namespace = self
            .namespaces
            .iter_mut()
            .find(|n| n.identity() == &self.enrollment.namespace)
            .ok_or(Error::Trust)?;
        let start = Instant::now();
        let mut selection = lifecycle::Execution {
            enrollment: self.enrollment,
            repository: &source,
            namespace,
            metadata: self.metadata,
            allow_selection: true,
        };
        let selection_plan = selection
            .plan(Some(self.request.commit()))
            .map_err(selection_error)?;
        let selected = selection.apply(&selection_plan).map_err(selection_error)?;
        let mut report = Report {
            schema: "tig.github-placement-report.design-1",
            provenance: self.request.provenance.clone(),
            selection: selected,
            pages: 0,
            objects: 0,
            stream_bytes: 0,
            effects: 0,
            basis: None,
            next: cursor.cloned(),
            holds: Vec::new(),
            storage_converged: Some(false),
        };
        if report.selection.observed_ref_current != Some(true) || !report.selection.desired_current
        {
            report.hold(Hold::Admission(Error::Stale));
            return Ok(report);
        }
        let mut complete = true;
        loop {
            let time = match now() {
                Ok(t) => t,
                Err(e) => {
                    report.hold(Hold::Admission(e));
                    break;
                }
            };
            if let Err(e) = self.request.admit(time, &selection_id, &placement_id) {
                report.hold(Hold::Admission(e));
                break;
            }
            if report.pages >= self.budget.pages
                || report.objects >= self.budget.objects
                || start.elapsed() >= Duration::from_millis(self.budget.millis)
            {
                report.hold(Hold::Admission(Error::Budget));
                break;
            }
            match source.named_commit(&self.enrollment.ref_name) {
                Ok(current) if &current == self.request.commit() => {}
                Ok(_) => {
                    report.hold(Hold::Admission(Error::Stale));
                    report.next = None;
                    break;
                }
                Err(e) => {
                    report.hold(Hold::Admission(selection_error(e)));
                    break;
                }
            }
            match selected_now(self.namespaces, self.enrollment, &selection_plan) {
                Ok(status) => {
                    report.selection = status;
                    if !report.selection.desired_current {
                        report.hold(Hold::Admission(Error::Stale));
                        report.next = None;
                        break;
                    }
                }
                Err(error) => {
                    report.hold(Hold::Admission(error));
                    break;
                }
            }
            let mut config = self.config.clone();
            config.budget.objects = config
                .budget
                .objects
                .min(self.budget.objects - report.objects);
            let remaining = self
                .budget
                .stream_bytes
                .checked_sub(report.stream_bytes)
                .ok_or(Error::Budget)?;
            config.budget.max_stream_bytes = config.budget.max_stream_bytes.min(remaining.max(1));
            config.budget.limits.millis = config.budget.limits.millis.min(
                self.budget
                    .millis
                    .saturating_sub(start.elapsed().as_millis() as u64)
                    .max(1),
            );
            let nodes = match self.metadata.finished_reader() {
                Ok(nodes) => nodes,
                Err(_) => {
                    report.hold(Hold::Admission(Error::Unavailable));
                    break;
                }
            };
            let mut execution = local::Execution {
                config: &config,
                namespaces: self.namespaces,
                nodes: &nodes,
                places: self.places,
                seed: self.seed,
                seed_identity: self.seed_identity,
                allow_copy: self.permissions.copy,
                allow_eviction: self.permissions.eviction,
                allow_reacquisition: self.permissions.reacquisition,
                allow_cancellation: self.permissions.cancellation,
            };
            let plan = match execution.plan(report.next.as_ref()) {
                Ok(p) => p,
                Err(e) => {
                    report.hold(Hold::Placement(e));
                    report.next = None;
                    break;
                }
            };
            if report.basis.as_ref().is_some_and(|b| b != &plan.basis) {
                report.hold(Hold::Admission(Error::Stale));
                report.next = None;
                break;
            }
            report.basis = Some(plan.basis.clone());
            let page = match if remaining == 0 {
                execution.status(&plan)
            } else {
                execution.apply(&plan)
            } {
                Ok(r) => r,
                Err(e) => {
                    report.hold(Hold::Placement(e));
                    break;
                }
            };
            report.pages += 1;
            report.objects += plan.rows.len();
            report.stream_bytes = report
                .stream_bytes
                .checked_add(page.stream_bytes)
                .ok_or(Error::Budget)?;
            report.effects = report
                .effects
                .checked_add(page.effects.len())
                .ok_or(Error::Budget)?;
            complete &= page.page_converged;
            for hold in page.holds {
                report.hold(Hold::Placement(hold));
            }
            if !page.basis_current {
                report.hold(Hold::Admission(Error::Stale));
                report.next = None;
                break;
            }
            if let Some(next) = &page.next
                && (next.basis != plan.basis
                    || plan
                        .after
                        .as_ref()
                        .is_some_and(|after| next.after <= *after))
            {
                return Err(Error::Invalid);
            }
            report.next = page.next;
            if report.next.is_none() {
                let current = match source.named_commit(&self.enrollment.ref_name) {
                    Ok(c) => c,
                    Err(e) => {
                        report.hold(Hold::Admission(selection_error(e)));
                        break;
                    }
                };
                if &current != self.request.commit() {
                    report.hold(Hold::Admission(Error::Stale));
                    break;
                }
                match selected_now(self.namespaces, self.enrollment, &selection_plan) {
                    Ok(status) => {
                        report.selection = status;
                        if !report.selection.desired_current {
                            report.hold(Hold::Admission(Error::Stale));
                            break;
                        }
                    }
                    Err(error) => {
                        report.hold(Hold::Admission(error));
                        break;
                    }
                }
                report.storage_converged = if complete && report.holds.is_empty() {
                    if cursor.is_none() { Some(true) } else { None }
                } else {
                    Some(false)
                };
                break;
            }
        }
        Ok(report)
    }
}
