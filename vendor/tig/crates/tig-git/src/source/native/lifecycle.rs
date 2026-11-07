//! Exact captured native history through an explicitly supplied bounded reader.
use super::AssetSnapshot;
use serde::Serialize;
use tig_assets::{
    delivery,
    native::Instance,
    revisions::{self, Coverage, Limits, Reader},
    schema::{Dialect, Schema},
};
use tig_core::{Digest, Object};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Missing,
    Integrity,
    Schema,
    Revision(revisions::Error),
    Delivery(delivery::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

enum Histories {
    Untracked,
    Revisions(Box<revisions::History>),
    Delivery(Box<delivery::History>),
}
pub struct Inspection {
    summary: Summary,
    histories: Histories,
    selected_subject: Option<Object>,
}
impl Inspection {
    pub fn selected_subject(&self) -> Option<&Object> {
        self.selected_subject.as_ref()
    }
    pub fn summary(&self) -> &Summary {
        &self.summary
    }
    pub fn revisions(&self) -> Option<&revisions::History> {
        match &self.histories {
            Histories::Revisions(h) => Some(h),
            Histories::Delivery(h) => h.revisions(),
            Histories::Untracked => None,
        }
    }
    pub fn delivery(&self) -> Option<&delivery::History> {
        match &self.histories {
            Histories::Delivery(h) => Some(h),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub schema: &'static str,
    pub source_checkpoint: Digest,
    pub record: Object,
    pub definition: Object,
    pub authored_schema: Object,
    pub schema_policy: Object,
    pub authored_schema_validated: bool,
    pub local_contracts_validated: bool,
    pub revision_history_coverage: Option<&'static str>,
    pub revisions: usize,
    pub selected_revision: Option<Object>,
    pub delivery_configured: bool,
    pub delivery_history_coverage: Option<&'static str>,
    pub delivery_events: usize,
    pub receipt_bindings: usize,
    pub metadata_objects: usize,
    pub receipt_objects: usize,
    pub opaque_witnesses: usize,
    pub targets: delivery::StateCounts,
    pub payload_verified: bool,
    pub current_remote_content_verified: bool,
    pub independent_custody_verified: bool,
    pub authorizes_eviction: bool,
    pub complete_semantic_coverage: bool,
}
fn coverage(value: Coverage) -> &'static str {
    match value {
        Coverage::Complete => "complete",
        Coverage::Partial => "partial",
    }
}

impl AssetSnapshot {
    /// Only this exact selected record's authored schema and common capabilities.
    /// No namespace-wide semantic/custody assertion or byte-store discovery.
    pub fn inspect_history(
        &self,
        id: &str,
        dialect: Dialect,
        limits: Limits,
        reader: &mut impl Reader,
    ) -> Result<Inspection, Error> {
        let reference = self.selected.get(id).ok_or(Error::Missing)?;
        let record = self.records.get(reference).ok_or(Error::Missing)?;
        let raw = self.blobs.get(&reference.object).ok_or(Error::Missing)?;
        let definition = self.blobs.get(&record.definition).ok_or(Error::Missing)?;
        let schema = self.blobs.get(&record.schema).ok_or(Error::Missing)?;
        let bound = Instance::parse(raw)
            .and_then(|i| i.bind(definition, schema))
            .map_err(|_| Error::Integrity)?;
        let checked = Schema::compile(bound.definition(), schema, dialect)
            .and_then(|s| s.check(&bound))
            .map_err(|_| Error::Schema)?;
        let histories = if bound.instance().value().metadata.contains_key("delivery") {
            Histories::Delivery(Box::new(
                delivery::check(&bound, &checked, limits, reader)
                    .map_err(Error::Delivery)?
                    .ok_or(Error::Integrity)?,
            ))
        } else {
            match revisions::check(&bound, &checked, limits, reader).map_err(Error::Revision)? {
                Some(h) => Histories::Revisions(Box::new(h)),
                None => Histories::Untracked,
            }
        };
        let mut inspection = Inspection {
            selected_subject: bound
                .instance()
                .value()
                .selected
                .as_ref()
                .map(|pin| pin.object()),
            summary: Summary {
                schema: "tig.asset-history-inspection.experimental-1",
                source_checkpoint: self.checkpoint().clone(),
                record: reference.object.clone(),
                definition: record.definition.clone(),
                authored_schema: record.schema.clone(),
                schema_policy: checked.policy().clone(),
                authored_schema_validated: true,
                local_contracts_validated: false,
                revision_history_coverage: None,
                revisions: 0,
                selected_revision: None,
                delivery_configured: false,
                delivery_history_coverage: None,
                delivery_events: 0,
                receipt_bindings: 0,
                metadata_objects: 0,
                receipt_objects: 0,
                opaque_witnesses: 0,
                targets: delivery::StateCounts::default(),
                payload_verified: false,
                current_remote_content_verified: false,
                independent_custody_verified: false,
                authorizes_eviction: false,
                complete_semantic_coverage: false,
            },
            histories,
        };
        if let Some(history) = inspection.revisions() {
            let counts = (
                history.coverage(),
                history.revision_count(),
                history.selected_revision().cloned(),
                history.documents().len(),
                history.provenance().len(),
            );
            inspection.summary.revision_history_coverage = Some(coverage(counts.0));
            inspection.summary.revisions = counts.1;
            inspection.summary.selected_revision = counts.2;
            inspection.summary.metadata_objects = counts.3;
            inspection.summary.opaque_witnesses = counts.4;
        }
        if let Some(history) = inspection.delivery() {
            let counts = (
                history.coverage(),
                history.event_count(),
                history.receipt_binding_count(),
                history.metadata().len(),
                history.receipts().len(),
                history.opaque().len(),
                history.state_counts(),
            );
            inspection.summary.delivery_configured = true;
            inspection.summary.delivery_history_coverage = counts.0.map(coverage);
            inspection.summary.delivery_events = counts.1;
            inspection.summary.receipt_bindings = counts.2;
            inspection.summary.metadata_objects = counts.3;
            inspection.summary.receipt_objects = counts.4;
            inspection.summary.opaque_witnesses = counts.5;
            inspection.summary.targets = counts.6;
        }
        Ok(inspection)
    }
}
