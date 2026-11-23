//! Explicit trusted minimums, never an inferred latest or completion authority.
use super::work::{self, Work};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{Digest, identity::parse_json};
use tig_store::ledger::{CapacityLedger, Checkpoint};
pub type Minimums = BTreeMap<Digest, Checkpoint>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: String,
    minimum_checkpoints: Minimums,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema: String,
    #[serde(rename = "work_digest")]
    _work_digest: Digest,
    #[serde(rename = "positive_phase_complete")]
    _positive_phase_complete: bool,
    completed: Vec<Value>,
    #[serde(rename = "pending")]
    _pending: Option<Value>,
    last_confirmed_minimum_checkpoints: Minimums,
    #[serde(default)]
    provider_profiles: Option<Vec<Digest>>,
    custody: String,
    removal_authorized: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionReport {
    pub schema: String,
    pub work_digest: Digest,
    pub selected_places: Vec<String>,
    pub claim_metadata_complete: bool,
    pub completed: Vec<Value>,
    pub pending: Option<Value>,
    pub last_confirmed_minimum_checkpoints: Minimums,
    pub witness_scope: String,
    pub enforcement: String,
    pub held_custody: bool,
    pub independent_failure_domains: String,
    pub removal_authorized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_issuance_profiles: Option<Vec<Digest>>,
}
fn profile_ids(ids: Option<&[Digest]>, required: bool) -> bool {
    match (required, ids) {
        (false, None) => true,
        (true, Some(ids)) => ids.len() <= 16 && ids.windows(2).all(|pair| pair[0] < pair[1]),
        _ => false,
    }
}
pub fn parse(bytes: &[u8]) -> Result<Minimums, String> {
    let value = parse_json(bytes).map_err(|_| "invalid or over-budget witness JSON")?;
    let provider_field = value.get("provider_profiles").is_some();
    let issuance_field = value.get("requested_issuance_profiles").is_some();
    let minimums = if value["schema"] == "tig.capacity-witnesses.experimental-1" {
        let file: File =
            serde_json::from_value(value).map_err(|_| "invalid witness file fields")?;
        if file.schema != "tig.capacity-witnesses.experimental-1" {
            return Err("unsupported witness schema".into());
        }
        file.minimum_checkpoints
    } else if matches!(
        value["schema"].as_str(),
        Some(
            "tig.retained-placement-result.experimental-1"
                | "tig.retained-placement-result.experimental-2"
        )
    ) {
        let report: RetentionReport =
            serde_json::from_value(value).map_err(|_| "invalid retention result fields")?;
        let profiled = report.schema == "tig.retained-placement-result.experimental-2";
        if (!profiled && issuance_field)
            || !profile_ids(report.requested_issuance_profiles.as_deref(), profiled)
            || report.completed.len() > 32
            || !(1..=16).contains(&report.selected_places.len())
            || report.selected_places.iter().any(|p| !work::token(p))
            || report.held_custody
            || report.independent_failure_domains != "unknown"
            || report.removal_authorized
            || report.witness_scope != "selected-retention-authorities"
            || report.enforcement != "cooperative-process-restart"
        {
            return Err("unsupported retention result".into());
        }
        report.last_confirmed_minimum_checkpoints
    } else {
        let report: Report =
            serde_json::from_value(value).map_err(|_| "invalid prior result fields")?;
        if !matches!(
            report.schema.as_str(),
            "tig.positive-placement-result.experimental-1"
                | "tig.positive-placement-result.experimental-2"
                | "tig.positive-placement-result.experimental-3"
        ) || (report.schema != "tig.positive-placement-result.experimental-3" && provider_field)
            || !profile_ids(
                report.provider_profiles.as_deref(),
                report.schema == "tig.positive-placement-result.experimental-3",
            )
            || report.completed.len() > 32
            || report.removal_authorized
            || report.custody != "unknown"
        {
            return Err("unsupported prior result".into());
        }
        // Completion claims and work identity are deliberately not authorization:
        // checkpoints belong to an authority and can inform another work recipe.
        report.last_confirmed_minimum_checkpoints
    };
    if minimums.len() > 32 {
        return Err("witness authority ceiling exceeded".into());
    }
    Ok(minimums)
}
pub fn check_scope(work: &Work, minimums: &Minimums) -> Result<(), String> {
    let mut authorities = BTreeSet::new();
    for job in &work.jobs {
        authorities.insert(work::input(job)?.capacity.ledger);
    }
    if minimums.len() > 32 || minimums.keys().any(|id| !authorities.contains(id)) {
        return Err("witness names an authority outside this work".into());
    }
    Ok(())
}
/// Best effort after a failed job: retain the old minimum if observation fails.
/// This adds no command and does not turn uncertain effects into NoEffect.
pub fn observe(job: &work::Job, minimums: &mut Minimums) {
    let Ok(input) = work::input(job) else {
        return;
    };
    let Some(minimum) = minimums.get(&input.capacity.ledger) else {
        return;
    };
    if let Ok(ledger) = CapacityLedger::open(&input.capacity.root, minimum)
        && ledger.identity() == &input.capacity.ledger
    {
        minimums.insert(input.capacity.ledger, ledger.checkpoint());
    }
}
