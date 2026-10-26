//! Explicit metadata-only consumer lookup. No media IO or configuration writes.
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
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};

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
    #[serde(default)]
    byte_source: Option<ByteProfile>,
}
fn scope(values: Vec<String>) -> Result<Scope, String> {
    let paths = values
        .iter()
        .map(|p| LogicalPath::parse(p))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "invalid scope".to_owned())?;
    Scope::new(paths).map_err(|e| e.to_string())
}
fn run() -> Result<(), String> {
    let mut args = env::args_os()
        .skip(1)
        .take(5)
        .map(|a| {
            a.into_string()
                .map_err(|_| "arguments must be Unicode".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let verify = args.first().is_some_and(|s| s == "--verify");
    if verify {
        args.remove(0);
    }
    if !matches!(args.len(), 2 | 3) {
        return Err("usage: tig-resolve [--verify] PROFILE ID [STREAM_SLOT]".into());
    }
    let mut bytes = Vec::new();
    File::open(&args[0])
        .map_err(|_| "cannot open explicit profile")?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read explicit profile")?;
    let profile: Profile =
        serde_json::from_value(parse_json(&bytes).map_err(|_| "invalid profile JSON")?)
            .map_err(|_| "invalid profile")?;
    if profile.schema != "tig.asset-resolver-profile.experimental-1" {
        return Err("unsupported profile schema".into());
    }
    let repository = Repository::new(
        profile.git_executable,
        profile.repository,
        GitLimits::default(),
    )
    .map_err(|e| e.to_string())?;
    let view = repository
        .commit(GitId::parse(&profile.commit).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let assets = repository
        .native(
            &view,
            scope(profile.records)?,
            scope(profile.definitions)?,
            profile.limits,
        )
        .map_err(|e| e.to_string())?;
    let asset = assets.resolve_id(&args[1]).map_err(|e| e.to_string())?;
    let object = if let Some(slot) = args.get(2) {
        asset.stream(slot)
    } else {
        asset.selected()
    }
    .map_err(|e| e.to_string())?;
    let mut held = None;
    if verify {
        let profile = profile
            .byte_source
            .ok_or("explicit byte_source profile required")?;
        held = Some(
            ReadCache::new(profile)
                .map_err(|e| e.to_string())?
                .open_verified(object)
                .map_err(|e| e.to_string())?,
        );
    }
    let output = serde_json::json!({"schema":"tig.asset-resolution.experimental-1",
        "source_checkpoint":asset.checkpoint(),"record":asset.reference(),"definition":asset.definition(),
        "object":object,"payload_verified":held.is_some(),"protection":held.as_ref().map(|r| r.protection()),
        "verified_source_index":held.as_ref().map(|r| r.place_index()),"complete_semantic_coverage":asset.complete_semantic_coverage()});
    writeln!(std::io::stdout().lock(), "{output}").map_err(|_| "cannot write resolution")?;
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
