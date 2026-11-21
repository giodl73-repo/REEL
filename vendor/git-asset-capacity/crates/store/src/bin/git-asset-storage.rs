//! Explicit profile storage operators. No implicit initialization or discovery.
#![forbid(unsafe_code)]
use git_asset_core::identity;
use git_asset_store::{
    accounted::AccountedError,
    ledger::{CapacityLedger, Checkpoint, JournalRef, LedgerError},
    local::LocalStore,
    orchestration::{self, Basis, LocalExecution, Place, Plan, PlanRequest},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

type Result<T> = std::result::Result<T, &'static str>;
#[path = "git-asset-storage/age.rs"]
mod age;
#[path = "git-asset-storage/bulk.rs"]
mod bulk;
#[path = "git-asset-storage/paged.rs"]
mod paged;
#[path = "git-asset-storage/retire.rs"]
mod retire;
#[cfg(feature = "s3")]
#[path = "git-asset-storage/s3.rs"]
mod s3;
const RESPONSE: &str = "tig.storage-response.design-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    source_root: PathBuf,
    destination_root: PathBuf,
    ledger_root: PathBuf,
    minimum: Checkpoint,
    job: JournalRef,
    source: Place,
    destination: Place,
    basis: Basis,
}
fn raw(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path).map_err(|_| "document_unavailable")?;
    if !meta.is_file() || meta.len() > 65536 {
        return Err("invalid_document");
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err("invalid_document");
        }
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let file = options.open(path).map_err(|_| "document_unavailable")?;
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "document_unavailable")?;
    if bytes.len() > 65536 {
        return Err("invalid_document");
    }
    Ok(bytes)
}
fn document<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let value = identity::parse_json(&raw(path)?).map_err(|_| "invalid_document")?;
    serde_json::from_value(value).map_err(|_| "invalid_document")
}
fn code(error: orchestration::Error) -> &'static str {
    use orchestration::Error;
    match error {
        Error::InvalidPlan => "invalid_plan",
        Error::Budget => "budget",
        Error::StaleAdmission => "stale_admission",
        Error::WrongStore => "wrong_store",
        Error::Store(git_asset_store::Error::AuthorizedAbsence)
        | Error::Publication(AccountedError::Store(git_asset_store::Error::AuthorizedAbsence)) => {
            "authorized_absence"
        }
        Error::Store(git_asset_store::Error::ReacquisitionRequired)
        | Error::Publication(AccountedError::Store(
            git_asset_store::Error::ReacquisitionRequired,
        )) => "reacquisition_required",
        Error::Store(git_asset_store::Error::NeedsRecovery)
        | Error::Publication(AccountedError::Store(git_asset_store::Error::NeedsRecovery)) => {
            "needs_recovery"
        }
        Error::Ledger(LedgerError::Conflict)
        | Error::Publication(AccountedError::Ledger(LedgerError::Conflict)) => "operation_conflict",
        Error::Ledger(LedgerError::Budget)
        | Error::Publication(AccountedError::Ledger(LedgerError::Budget)) => "journal_budget",
        Error::Ledger(LedgerError::NeedsRecovery) => "needs_recovery",
        Error::Ledger(LedgerError::Cancelled)
        | Error::Publication(AccountedError::Ledger(LedgerError::Cancelled)) => "cancelled",
        Error::Publication(AccountedError::PendingPartial) => "private_partial_requires_review",
        _ => "operation_failed",
    }
}
fn report(value: Value) -> Result<Vec<u8>> {
    identity::canonical_json(&value).map_err(|_| "response_budget")
}
fn run(args: &[String]) -> Result<Vec<u8>> {
    if args.first().is_some_and(|s| s == "paged") {
        return paged::run(&args[1..]);
    }
    if args.first().is_some_and(|s| s == "age") {
        return age::run(&args[1..]);
    }
    if args.first().is_some_and(|s| s == "bulk") {
        return bulk::run(&args[1..]);
    }
    #[cfg(feature = "s3")]
    if args.first().is_some_and(|s| s == "s3") {
        return s3::run(&args[1..]);
    }
    if args.first().is_some_and(|s| s == "retire") {
        return retire::run(&args[1..]);
    }
    match args.first().map(String::as_str) {
        Some("describe") if args.len() == 1 => report(json!({
            "schema": RESPONSE, "operations": ["plan REQUEST", "peak PROFILE PLAN", "status PROFILE PLAN INDEX", "apply PROFILE PLAN INDEX", "resume PROFILE PLAN INDEX"],
            "max_actions": orchestration::MAX_ACTIONS, "max_document_bytes": 65536,
            "copy_only": true, "removal_authority": false,
            "profile_schema": "tig.storage-profile.design-1", "additional_interfaces":{"age":"git-asset-storage age describe","bulk":"git-asset-storage bulk describe","paged":"git-asset-storage paged describe","retire":"git-asset-storage retire describe","s3":{"command":"git-asset-storage s3 describe","compiled":cfg!(feature="s3")}}
        })),
        Some("plan") if args.len() == 2 => {
            let request: PlanRequest = document(Path::new(&args[1]))?;
            Plan::new(request).map_err(code)?.to_json().map_err(code)
        }
        Some(action @ ("peak" | "status" | "apply" | "resume")) => {
            if (action == "peak" && args.len() != 3) || (action != "peak" && args.len() != 4) {
                return Err("usage");
            }
            // Parse/index validation before opening any enrolled provider.
            let plan = Plan::from_json(&raw(Path::new(&args[2]))?).map_err(code)?;
            let index = if action != "peak" {
                let index = args[3].parse::<usize>().map_err(|_| "usage")?;
                if index >= plan.request().copies.len() {
                    return Err("invalid_index");
                }
                Some(index)
            } else {
                None
            };
            let profile: Profile = document(Path::new(&args[1]))?;
            if profile.schema != "tig.storage-profile.design-1"
                || !profile.source_root.is_absolute()
                || !profile.destination_root.is_absolute()
                || !profile.ledger_root.is_absolute()
            {
                return Err("invalid_profile");
            }
            if profile.source != plan.request().source
                || profile.destination != plan.request().destination
                || profile.basis != plan.request().basis
            {
                return Err("stale_admission");
            }
            let source =
                LocalStore::open(&profile.source_root).map_err(|_| "source_unavailable")?;
            let destination = LocalStore::open(&profile.destination_root)
                .map_err(|_| "destination_unavailable")?;
            let mut ledger = CapacityLedger::open(&profile.ledger_root, &profile.minimum)
                .map_err(|_| "ledger_unavailable")?;
            if source.identity() != &profile.source.instance
                || destination.identity() != &profile.destination.instance
                || ledger.identity() != &plan.request().capacity
            {
                return Err("wrong_store");
            }
            if action == "peak" {
                return report(
                    json!({"schema":RESPONSE,"plan":plan.id(),"peak_file_data_bytes":plan.local_peak(&destination).map_err(code)?,
                    "holds":plan.holds(),"removal_authority":false}),
                );
            }
            let index = index.ok_or("invalid_index")?;
            let mut execution = LocalExecution {
                source: &source,
                destination: &destination,
                ledger: &mut ledger,
                admitted_source: &profile.source,
                admitted_destination: &profile.destination,
                admitted_basis: &profile.basis,
            };
            if action == "status" {
                let status = execution.status(&plan, index, &profile.job).map_err(code)?;
                return report(
                    json!({"schema":RESPONSE,"plan":plan.id(),"operation":plan.request().copies[index].operation,
                    "phase":status.phase,"holds":plan.holds(),"removal_authority":false}),
                );
            }
            let receipt = execution.apply(&plan, index, &profile.job).map_err(code)?;
            report(
                json!({"schema":RESPONSE,"plan":receipt.plan,"operation":receipt.operation,"phase":"retained",
                "publication_file_data_bytes":receipt.publication_footprint,"retention_file_data_bytes":receipt.retention_footprint,
                "acknowledgement":"process_restart","removal_authority":false,"source_removed":false}),
            )
        }
        _ => Err("usage"),
    }
}
fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (bytes, exit) = match run(&args) {
        Ok(bytes) => (bytes, ExitCode::SUCCESS),
        Err(code) => (
            report(json!({"schema":RESPONSE,"ok":false,"code":code})).unwrap_or_default(),
            ExitCode::FAILURE,
        ),
    };
    if std::io::stdout().lock().write_all(&bytes).is_err() {
        return ExitCode::FAILURE;
    }
    exit
}
