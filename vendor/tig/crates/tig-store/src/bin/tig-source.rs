//! Explicit existing-authority inspection and source-claim transitions.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};
use tig_core::{Digest, identity::parse_json};
use tig_store::{
    Key,
    source_claims::{Checkpoint, Command, SourceJournal},
};

const LIMIT: usize = 65_536;
#[path = "tig-source/transition.rs"]
mod transition;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    journal_root: PathBuf,
    minimum: Checkpoint,
    source: Key,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CommandProfile {
    schema: String,
    journal_root: PathBuf,
    minimum: Checkpoint,
    operation: Key,
    command: Command,
}
enum Action {
    Inspect(Key),
    Acknowledge(Key, Command),
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn read_value(input: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    File::open(input)?
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(parse_json(&bytes)?)
}
fn validate_output(root: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let parent = output.parent().ok_or("output parent missing")?;
    if !root.is_absolute()
        || !output.is_absolute()
        || output.symlink_metadata().is_ok()
        || !fs::symlink_metadata(parent)?.is_dir()
        || parent.canonicalize()?.starts_with(root.canonicalize()?)
    {
        return Err("invalid output settings".into());
    }
    Ok(())
}
fn run(input: &Path, output: &Path, acknowledge: bool) -> Result<(), Box<dyn std::error::Error>> {
    let value = read_value(input)?;
    let (root, minimum, action) = if acknowledge {
        let profile: CommandProfile = serde_json::from_value(value)?;
        if profile.schema != "tig.source-acknowledgment-profile.experimental-1" {
            return Err("invalid acknowledgment schema".into());
        }
        (
            profile.journal_root,
            profile.minimum,
            Action::Acknowledge(profile.operation, profile.command),
        )
    } else {
        let profile: Profile = serde_json::from_value(value)?;
        if profile.schema != "tig.source-observation-profile.experimental-1" {
            return Err("invalid observation schema".into());
        }
        (
            profile.journal_root,
            profile.minimum,
            Action::Inspect(profile.source),
        )
    };
    validate_output(&root, output)?;
    let (summary, report) = match action {
        Action::Inspect(source) => {
            let observed = SourceJournal::observe(&root, &minimum, &source)?;
            let summary = json!({"schema":"tig.source-observation-summary.experimental-1",
        "source_exists":observed.source.is_some(),"journal_commands":observed.checkpoint.sequence(),
        "source_revision":observed.source.as_ref().map(|source|source.revision),
        "current_roots":observed.source.as_ref().map_or(0,|source|usize::from(source.current.is_some())),
        "pending_roots":observed.source.as_ref().map_or(0,|source|usize::from(source.pending.is_some())),
        "predecessor_roots":observed.source.as_ref().map_or(0,|source|source.predecessors.len()),
        "recovery_authorized":false,"complete_claim_coverage":false,"authorizes_eviction":false});
            let report =
                serde_json::to_vec(&json!({"schema":"tig.source-observation.experimental-1",
        "journal_root":root,"source_id":source,
        "checkpoint":observed.checkpoint,"source":observed.source,
        "observation_only":true,"process_liveness_verified":false,"recovery_authorized":false,
        "complete_claim_coverage":false,"authorizes_eviction":false}))?;
            (summary, report)
        }
        Action::Acknowledge(operation, command) => {
            let receipt = SourceJournal::acknowledge(&root, &minimum, operation, command)?;
            let summary = json!({"schema":"tig.source-acknowledgment-summary.experimental-1",
            "recorded":true,"historical_source_revision":receipt.source_revision,
            "historical_journal_commands":receipt.checkpoint.sequence(),
            "transition_executed":false,"producer_success_verified":false,"authorizes_eviction":false});
            let report = serde_json::to_vec(
                &json!({"schema":"tig.source-acknowledgment.experimental-1",
            "journal_root":root,"reference":receipt.reference,"checkpoint":receipt.checkpoint,
            "source_revision":receipt.source_revision,"replayed":receipt.replayed,
            "historical_receipt":true,"transition_executed":false,
            "producer_success_verified":false,"complete_claim_coverage":false,"authorizes_eviction":false}),
            )?;
            (summary, report)
        }
    };
    publish(&report, output)?;
    println!("{summary}");
    Ok(())
}
fn publish(report: &[u8], output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if report.len() > LIMIT {
        return Err("observation exceeds budget".into());
    }
    let mut nonce = [0; 32];
    getrandom::fill(&mut nonce)?;
    let parent = output.parent().ok_or("output parent missing")?;
    let temporary_path = parent.join(format!(
        ".tig-observation-{}.tmp",
        Digest::blob(&nonce).as_str()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)?;
    let temporary = Temporary(temporary_path);
    file.write_all(report)?;
    file.sync_all()?;
    drop(file);
    // Same-parent hard-link publication never replaces an existing destination.
    // Unsupported filesystems refuse safely; only this owned temporary is removed.
    fs::hard_link(&temporary.0, output)?;
    drop(temporary);
    Ok(())
}
fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).take(5).collect();
    let result = if args.len() == 3 && (args[0] == "inspect" || args[0] == "acknowledge") {
        run(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args[0] == "acknowledge",
        )
    } else if args.len() == 3 && args[0] == "plan" {
        transition::plan(Path::new(&args[1]), Path::new(&args[2]))
    } else if args.len() == 4 && args[0] == "apply" {
        transition::apply(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
        )
    } else {
        eprintln!(
            "usage: tig-source inspect|acknowledge|plan PRIVATE_PROFILE NEW_PRIVATE_REPORT; or apply PRIVATE_PLAN NEW_PRIVATE_REQUEST NEW_PRIVATE_REPORT"
        );
        return ExitCode::FAILURE;
    };
    if result.is_err() {
        eprintln!(
            "source request refused; verify authority, trusted minimum, exact request, bounds and new output"
        );
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
