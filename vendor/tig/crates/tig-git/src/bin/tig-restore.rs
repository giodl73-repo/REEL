//! Explicit one-object restoration from an exact Git view and enrolled store.
#![forbid(unsafe_code)]
use serde::Deserialize;
use std::{
    env,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    process::ExitCode,
};
use tig_assets::resolve::AssetResolver;
use tig_core::{LogicalPath, identity::parse_json};
use tig_git::source::{GitId, Limits as GitLimits, Repository, Scope, native::Limits};
use tig_store::existing::{Profile as ByteProfile, ReadCache};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    git_executable: PathBuf,
    repository: PathBuf,
    commit: String,
    records: Vec<String>,
    definitions: Vec<String>,
    #[serde(default)]
    limits: Limits,
    byte_source: ByteProfile,
}
fn scope(paths: Vec<String>) -> Result<Scope, String> {
    Scope::new(
        paths
            .iter()
            .map(|p| LogicalPath::parse(p))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "invalid scope")?,
    )
    .map_err(|e| e.to_string())
}
fn run() -> Result<(), String> {
    let usage = "usage: tig-restore [--apply] [--selected] PROFILE ID [SLOT] ABSOLUTE_DESTINATION";
    let mut args: Vec<_> = env::args_os().skip(1).take(7).collect();
    let mut apply = false;
    let mut selected = false;
    while let Some(flag) = args.first() {
        if flag == "--apply" && !apply {
            apply = true;
        } else if flag == "--selected" && !selected {
            selected = true;
        } else if flag.to_string_lossy().starts_with("--") {
            return Err(usage.into());
        } else {
            break;
        }
        args.remove(0);
    }
    if args.len() != if selected { 3 } else { 4 } {
        return Err(usage.into());
    }
    let mut raw = Vec::new();
    File::open(&args[0])
        .map_err(|_| "cannot open explicit profile")?
        .take(65537)
        .read_to_end(&mut raw)
        .map_err(|_| "cannot read explicit profile")?;
    if raw.len() > 65536 {
        return Err("profile too large".into());
    }
    let p: Profile = serde_json::from_value(parse_json(&raw).map_err(|_| "invalid profile JSON")?)
        .map_err(|_| "invalid profile")?;
    if p.schema != "tig.asset-resolver-profile.experimental-1" {
        return Err("unsupported profile schema".into());
    }
    let repo = Repository::new(p.git_executable, p.repository, GitLimits::default())
        .map_err(|e| e.to_string())?;
    let view = repo
        .commit(GitId::parse(&p.commit).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let assets = repo
        .native(&view, scope(p.records)?, scope(p.definitions)?, p.limits)
        .map_err(|e| e.to_string())?;
    let asset = assets
        .resolve_id(args[1].to_str().ok_or("invalid ID")?)
        .map_err(|e| e.to_string())?;
    let object = if selected {
        asset.selected()
    } else {
        asset.stream(args[2].to_str().ok_or("invalid slot")?)
    }
    .map_err(|e| e.to_string())?;
    let mut cache = ReadCache::new(p.byte_source).map_err(|e| e.to_string())?;
    let dest = PathBuf::from(&args[if selected { 2 } else { 3 }]);
    let outcome = tig_git::restore::restore(&mut cache, object, &dest, apply)?;
    let action = outcome.action;
    let result = serde_json::json!({"schema":"tig.restore-result.experimental-1", "action":action,
        "source_checkpoint":asset.checkpoint(), "object":object, "payload_verified":true,
        "payload_custody":false, "complete_semantic_coverage":false});
    writeln!(std::io::stdout().lock(), "{result}").map_err(|_| "cannot write result")?;
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
