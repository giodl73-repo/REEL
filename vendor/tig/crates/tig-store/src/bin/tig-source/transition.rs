//! Explicit trusted-caller metadata operations; never payload eviction.
use super::{CommandProfile, publish, read_value, validate_output};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use tig_core::{Digest, identity::canonical_json};
use tig_store::source_claims::{Preview, PreviewDisposition, SourceJournal};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const REQUEST_SCHEMA: &str = "tig.source-transition-profile.experimental-1";
const PLAN_SCHEMA: &str = "tig.source-transition-plan.experimental-1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: String,
    request: CommandProfile,
    request_sha256: Digest,
    preview: Preview,
    dry_run: bool,
    complete_claim_coverage: bool,
    authorizes_eviction: bool,
}
fn request_hash(request: &CommandProfile) -> Result<Digest> {
    Ok(Digest::blob(&canonical_json(&serde_json::to_value(
        request,
    )?)?))
}
pub(super) fn plan(input: &Path, output: &Path) -> Result<()> {
    let request: CommandProfile = serde_json::from_value(read_value(input)?)?;
    if request.schema != REQUEST_SCHEMA {
        return Err("invalid transition profile".into());
    }
    validate_output(&request.journal_root, output)?;
    let preview = SourceJournal::preview(
        &request.journal_root,
        &request.minimum,
        request.operation.clone(),
        request.command.clone(),
    )?;
    let recorded = matches!(preview.disposition, PreviewDisposition::Recorded { .. });
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        request_sha256: request_hash(&request)?,
        request,
        preview,
        dry_run: true,
        complete_claim_coverage: false,
        authorizes_eviction: false,
    };
    publish(&serde_json::to_vec(&plan)?, output)?;
    println!(
        "{}",
        json!({"schema":"tig.source-transition-plan-summary.experimental-1",
        "dry_run":true,"already_recorded":recorded,"transition_executed":false,
        "complete_claim_coverage":false,"authorizes_eviction":false})
    );
    Ok(())
}
pub(super) fn apply(input: &Path, intent: &Path, output: &Path) -> Result<()> {
    let plan: Plan = serde_json::from_value(read_value(input)?)?;
    if plan.schema != PLAN_SCHEMA
        || plan.request.schema != REQUEST_SCHEMA
        || !plan.dry_run
        || plan.complete_claim_coverage
        || plan.authorizes_eviction
        || request_hash(&plan.request)? != plan.request_sha256
    {
        return Err("invalid transition plan".into());
    }
    let request = plan.request;
    if intent == output {
        return Err("intent and receipt paths must differ".into());
    }
    validate_output(&request.journal_root, intent)?;
    validate_output(&request.journal_root, output)?;
    // The plan's observed checkpoint is a stronger rollback floor. Re-preview
    // admits unrelated progress but rechecks owner/revision/operation/body.
    let fresh = SourceJournal::preview(
        &request.journal_root,
        &plan.preview.checkpoint,
        request.operation.clone(),
        request.command.clone(),
    )?;
    if matches!(fresh.disposition, PreviewDisposition::Proposed { .. })
        && fresh.disposition != plan.preview.disposition
    {
        return Err("proposed transition differs from reviewed plan".into());
    }
    let saved = CommandProfile {
        schema: "tig.source-acknowledgment-profile.experimental-1".into(),
        minimum: fresh.checkpoint.clone(),
        ..request.clone()
    };
    publish(&serde_json::to_vec(&saved)?, intent)?;
    // Detect aliasing with the saved intent before any transition. A later
    // receipt publication race/failure is recovered via this exact saved request.
    validate_output(&request.journal_root, output)?;
    let receipt = match fresh.disposition {
        PreviewDisposition::Recorded { .. } => SourceJournal::acknowledge(
            &request.journal_root,
            &fresh.checkpoint,
            request.operation,
            request.command,
        )?,
        PreviewDisposition::Proposed { .. } => {
            SourceJournal::open(&request.journal_root, &fresh.checkpoint)?
                .apply(request.operation, request.command)?
        }
    };
    publish(
        &serde_json::to_vec(
            &json!({"schema":"tig.source-transition-result.experimental-1",
        "plan_request_sha256":plan.request_sha256,"receipt":receipt,
        "transition_executed":!receipt.replayed,"producer_success_verified":false,
        "complete_claim_coverage":false,"authorizes_eviction":false}),
        )?,
        output,
    )?;
    println!(
        "{}",
        json!({"schema":"tig.source-transition-summary.experimental-1",
        "historical_source_revision":receipt.source_revision,"replayed":receipt.replayed,
        "transition_executed":!receipt.replayed,"complete_claim_coverage":false,"authorizes_eviction":false})
    );
    Ok(())
}
