//! Explicit trusted operator entry, not an authenticated GitHub webhook receiver.
#[path = "lifecycle/index.rs"]
mod index_cli;
#[path = "lifecycle/local_placement.rs"]
mod local_placement;
#[path = "lifecycle/placement.rs"]
mod placement_cli;
#[path = "lifecycle/profiles.rs"]
mod profiles;
#[path = "lifecycle/settlement.rs"]
mod settlement_cli;
use git_asset::{
    lifecycle::{self, Enrollment, Execution, Plan},
    source::{GitId, Limits, Repository},
};
use git_asset_core::{Digest, identity};
use git_asset_store::{
    metadata::{self, MetadataStore},
    source_claims::{self, admission::Namespace},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    enrollment: Enrollment,
    git_executable: PathBuf,
    repository_root: PathBuf,
    source_limits: Limits,
    namespace_root: PathBuf,
    namespace_minimum: source_claims::Checkpoint,
    metadata_root: PathBuf,
    metadata_minimum: metadata::Checkpoint,
    #[serde(default)]
    allow_selection: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    repository: Digest,
    ref_name: String,
    commit: Option<String>,
}
fn absolute(path: &Path) -> Result<(), &'static str> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
    {
        return Err("invalid_document");
    }
    Ok(())
}
fn read(path: &Path) -> Result<Vec<u8>, &'static str> {
    absolute(path)?;
    let m = fs::symlink_metadata(path).map_err(|_| "invalid_document")?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > 65536 {
        return Err("invalid_document");
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err("invalid_document");
        }
    }
    let mut b = Vec::new();
    fs::File::open(path)
        .map_err(|_| "invalid_document")?
        .take(65537)
        .read_to_end(&mut b)
        .map_err(|_| "invalid_document")?;
    if b.len() > 65536 {
        return Err("budget");
    }
    Ok(b)
}
fn code(e: lifecycle::Error) -> &'static str {
    use lifecycle::Error::*;
    match e {
        Invalid => "invalid_document",
        Authority => "authority",
        Stale => "stale",
        Pending => "pending",
        Disabled => "selection_disabled",
        Budget => "budget",
        Git => "git_unavailable",
        Metadata => "metadata_unavailable",
        Journal => "journal_unavailable",
        Busy => "busy",
        Recovery => "needs_recovery",
    }
}
fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, &'static str> {
    serde_json::from_value(identity::parse_json(bytes).map_err(|_| "invalid_document")?)
        .map_err(|_| "invalid_document")
}
fn run() -> Result<Value, &'static str> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() == 1 && args[0] == "describe" {
        return Ok(
            json!({"schema":"tig.git-lifecycle-interface.design-1","profile_schema":"tig.git-lifecycle-profile.design-1","request_schema":"tig.git-lifecycle-request.design-1","source_format":"git_lfs_pointers","settlement_profile_schema":"tig.source-settlement-profile.design-1","settlement_operations":["settle plan PROFILE","settle status PROFILE PLAN","settle apply PROFILE PLAN","settle resume PROFILE PLAN","settle reconcile PROFILE PLACEMENT_PROFILE [CURSOR]"],"index_profile_schema":"tig.git-index-profile.design-1","index_operations":["index plan PROFILE","index status PROFILE PLAN","index apply PROFILE PLAN","index resume PROFILE PLAN","index reconcile PROFILE PLACEMENT_PROFILE [CURSOR]"],"operations":["plan PROFILE REQUEST","status PROFILE PLAN","apply PROFILE PLAN","resume PROFILE PLAN","reconcile PROFILE REQUEST PLACEMENT_PROFILE [CURSOR]"],"selection_only":false,"selection_commands":["plan","status","apply","resume"],"physical_reconciliation":true,"storage_converged":null,"default_selection":false,"authenticates_webhooks":false,"initializes_providers":false}),
        );
    }
    if args.first().is_some_and(|a| a == "reconcile") {
        return placement_cli::reconcile(&args);
    }
    if args.first().is_some_and(|a| a == "index") {
        return index_cli::run(&args);
    }
    if args.first().is_some_and(|a| a == "settle") {
        return settlement_cli::run(&args);
    }
    if args.len() != 3 {
        return Err("usage");
    }
    let action = args[0].to_str().ok_or("usage")?;
    if !matches!(action, "plan" | "status" | "apply" | "resume") {
        return Err("usage");
    }
    let profile: Profile = parse(&read(Path::new(&args[1]))?)?;
    if profile.schema != "tig.git-lifecycle-profile.design-1" {
        return Err("invalid_document");
    }
    for path in [
        &profile.git_executable,
        &profile.repository_root,
        &profile.namespace_root,
        &profile.metadata_root,
    ] {
        absolute(path)?;
    }
    profile.enrollment.id().map_err(code)?;
    let mut namespace = Namespace::open(&profile.namespace_root, &profile.namespace_minimum)
        .map_err(|_| "journal_unavailable")?;
    let bytes = read(Path::new(&args[2]))?;
    let existing = if action == "plan" {
        None
    } else {
        Some(Plan::from_json(&bytes).map_err(code)?)
    };
    if action == "status" {
        return serde_json::to_value(
            lifecycle::inspect_selection(
                &profile.enrollment,
                &mut namespace,
                existing.as_ref().ok_or("usage")?,
            )
            .map_err(code)?,
        )
        .map_err(|_| "invalid_document");
    }
    if action != "plan" && !profile.allow_selection {
        return Err("selection_disabled");
    }
    let repository = Repository::new(
        profile.git_executable,
        profile.repository_root,
        profile.source_limits,
    )
    .map_err(|_| "git_unavailable")?;
    let mut metadata = MetadataStore::open(&profile.metadata_root, &profile.metadata_minimum)
        .map_err(|_| "metadata_unavailable")?;
    let mut execution = Execution {
        enrollment: &profile.enrollment,
        repository: &repository,
        namespace: &mut namespace,
        metadata: &mut metadata,
        allow_selection: profile.allow_selection,
    };
    if action == "plan" {
        let request: Request = parse(&bytes)?;
        if request.schema != "tig.git-lifecycle-request.design-1"
            || request.repository != profile.enrollment.repository
            || request.ref_name != profile.enrollment.ref_name
        {
            return Err("authority");
        }
        let commit = request
            .commit
            .as_deref()
            .map(GitId::parse)
            .transpose()
            .map_err(|_| "invalid_document")?;
        return serde_json::to_value(execution.plan(commit.as_ref()).map_err(code)?)
            .map_err(|_| "invalid_document");
    }
    serde_json::to_value(
        execution
            .apply(existing.as_ref().ok_or("usage")?)
            .map_err(code)?,
    )
    .map_err(|_| "invalid_document")
}
fn main() {
    let (v, failed) = match run() {
        Ok(v) => (v, false),
        Err(code) => (
            json!({"schema":"tig.git-lifecycle-response.design-1","ok":false,"code":code}),
            true,
        ),
    };
    let Ok(b) = identity::canonical_json(&v) else {
        std::process::exit(1)
    };
    if std::io::stdout().lock().write_all(&b).is_err() || failed {
        std::process::exit(1);
    }
}
