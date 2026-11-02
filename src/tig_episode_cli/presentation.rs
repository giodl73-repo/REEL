//! Typed still/browser presentation dispatch; proofs share the sealed parent port.
use super::*;
#[cfg(windows)]
use crate::media_runtime::BoundTool;
use crate::tig_presentation;

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum Request {
    Still(tig_presentation::Request),
    #[cfg(windows)]
    Browser(crate::tig_browser::Request),
}
impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> std::result::Result<Self, D::Error> {
        let value = Value::deserialize(reader)?;
        match value.get("schema").and_then(Value::as_str) {
            Some(tig_presentation::SCHEMA) => serde_json::from_value(value)
                .map(Self::Still)
                .map_err(serde::de::Error::custom),
            #[cfg(windows)]
            Some(crate::tig_browser::SCHEMA) => serde_json::from_value(value)
                .map(Self::Browser)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(
                "unsupported episode presentation request",
            )),
        }
    }
}
impl Request {
    pub(super) fn definitions(&self) -> std::collections::BTreeSet<&str> {
        match self {
            Self::Still(r) => [
                r.source_definition.as_str(),
                r.template_definition.as_str(),
                r.timing_definition.as_str(),
                r.clock_definition.as_str(),
                r.evidence_definition.as_str(),
            ]
            .into_iter()
            .chain(r.primary_definitions.iter().map(String::as_str))
            .collect(),
            #[cfg(windows)]
            Self::Browser(r) => std::iter::once(r.root_definition.as_str())
                .chain(r.primary_definitions.iter().map(String::as_str))
                .collect(),
        }
    }
}
#[cfg(windows)]
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Execution {
    program: crate::scene_delivery::FileRef,
    limits: crate::tig_browser::render::Limits,
    clean_picture: bool,
}
impl Presentation {
    pub(super) fn freeze_context(
        &self,
        source: &mut Source,
        store: &mut impl NodeStore,
    ) -> Result<ProductionContext> {
        match &self.request {
            Request::Still(request) => {
                tig_presentation::freeze_context(source, request, store, TreeLimits::default())
            }
            #[cfg(windows)]
            Request::Browser(request) => Ok(crate::tig_browser::freeze_context(
                source,
                request,
                store,
                TreeLimits::default(),
            )?
            .1),
        }
    }
    pub(super) fn bind_browser(&self) -> Result<BrowserProgram> {
        #[cfg(windows)]
        match (&self.request, &self.browser) {
            (Request::Still(_), None) => Ok(None),
            (Request::Browser(_), Some(execution)) => {
                execution.limits.validate()?;
                Ok(Some(BoundTool::bind(&execution.program)?))
            }
            _ => bail!("episode presentation kind and browser admission differ"),
        }
        #[cfg(not(windows))]
        Ok(BrowserProgram)
    }
}
#[cfg(windows)]
pub(super) type BrowserProgram = Option<BoundTool>;
#[cfg(not(windows))]
pub(super) struct BrowserProgram;

pub(super) enum Plan<'a> {
    Still(tig_presentation::jobs::JobPlan<'a>),
    #[cfg(windows)]
    Browser(tig_presentation::jobs::browser::JobPlan<'a>),
}
pub(super) enum Readback<'p, 'a> {
    Still(tig_presentation::jobs::Readback<'p, 'a>),
    #[cfg(windows)]
    Browser(tig_presentation::jobs::browser::Readback<'p, 'a>),
}
pub(super) enum Owner<'p, 'a> {
    Still(tig_presentation::jobs::ProtectedPresentation<'p, 'a>),
    #[cfg(windows)]
    Browser(tig_presentation::jobs::browser::ProtectedPresentation<'p, 'a>),
}
impl<'a> Plan<'a> {
    pub(super) fn from_context(
        source: &mut Source,
        spec: &'a Presentation,
        context: &'a ProductionContext,
        programs: &'a BoundPrograms,
        browser: &'a BrowserProgram,
    ) -> Result<Self> {
        #[cfg(not(windows))]
        let _ = browser;
        match &spec.request {
            Request::Still(request) => {
                Ok(Self::Still(tig_presentation::jobs::JobPlan::from_context(
                    source,
                    request,
                    context,
                    programs,
                    &spec.output_id,
                    &spec.output_definition,
                )?))
            }
            #[cfg(windows)]
            Request::Browser(request) => {
                let execution = spec
                    .browser
                    .as_ref()
                    .context("browser execution admission missing")?;
                Ok(Self::Browser(
                    tig_presentation::jobs::browser::JobPlan::from_context(
                        source,
                        request,
                        context,
                        programs,
                        browser
                            .as_ref()
                            .context("protected browser program missing")?,
                        &execution.limits,
                        execution.clean_picture,
                        &spec.output_id,
                        &spec.output_definition,
                    )?,
                ))
            }
        }
    }
    pub(super) fn freeze(
        &self,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<job::JobContext> {
        match self {
            Self::Still(plan) => plan.freeze(store, limits),
            #[cfg(windows)]
            Self::Browser(plan) => plan.freeze(store, limits),
        }
    }
    pub(super) fn readback<'p>(
        &'p self,
        selection: &Selection,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<Readback<'p, 'a>> {
        match self {
            Self::Still(plan) => Ok(Readback::Still(plan.readback(selection, store, limits)?)),
            #[cfg(windows)]
            Self::Browser(plan) => Ok(Readback::Browser(plan.readback(selection, store, limits)?)),
        }
    }
}
impl<'p, 'a> Readback<'p, 'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn protect(
        self,
        source: &Source,
        journal: &mut SourceJournal,
        coordination: &Coordination,
        identity: Identity,
        revision: u64,
        operation: &Key,
        budget: &ProtectionBudget,
    ) -> Result<Owner<'p, 'a>> {
        match self {
            Self::Still(readback) => Ok(Owner::Still(readback.protect(
                source,
                journal,
                coordination,
                identity,
                revision,
                operation,
                budget,
            )?)),
            #[cfg(windows)]
            Self::Browser(readback) => Ok(Owner::Browser(readback.protect(
                source,
                journal,
                coordination,
                identity,
                revision,
                operation,
                budget,
            )?)),
        }
    }
}
impl Owner<'_, '_> {
    pub(super) fn verify_selected_master<'p>(
        &'p self,
        source: &mut Source,
        journal: &mut SourceJournal,
        stage: &Path,
        budget: u64,
    ) -> Result<tig_presentation::jobs::VerifiedPresentation<'p>> {
        match self {
            Self::Still(owner) => owner.verify_selected_master(source, journal, stage, budget),
            #[cfg(windows)]
            Self::Browser(owner) => owner.verify_selected_master(source, journal, stage, budget),
        }
    }
}
