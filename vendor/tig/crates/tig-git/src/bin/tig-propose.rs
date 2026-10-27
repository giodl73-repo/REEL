//! Explicit verified-cache -> small Git proposal handoff. No checkout or setup.
#![forbid(unsafe_code)]
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    process::ExitCode,
};
use tig_core::{LogicalPath, Object, identity::parse_json};
use tig_git::source::{
    GitId, Limits, Repository,
    proposal::{Identity, Replacement},
};
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    path: String,
    expected: Object,
    object: Object,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    git_executable: PathBuf,
    repository: PathBuf,
    base: String,
    identity: Identity,
    message: String,
    byte_source: ByteProfile,
    changes: Vec<Change>,
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).take(4).collect();
    if args.len() != 2 || (args[0] != "check" && args[0] != "create") {
        return Err("usage: tig-propose check|create PROFILE".into());
    }
    let create = args[0] == "create";
    let mut raw = Vec::new();
    File::open(&args[1])
        .map_err(|_| "cannot open explicit profile")?
        .take(65_537)
        .read_to_end(&mut raw)
        .map_err(|_| "cannot read explicit profile")?;
    if raw.len() > 65_536 {
        return Err("proposal profile exceeds budget".into());
    }
    let profile: Profile =
        serde_json::from_value(parse_json(&raw).map_err(|_| "invalid profile JSON")?)
            .map_err(|_| "invalid proposal profile")?;
    if profile.schema != "tig.git-proposal-profile.experimental-1"
        || profile.changes.is_empty()
        || profile.changes.len() > 128
    {
        return Err("unsupported or oversized proposal profile".into());
    }
    let mut names = BTreeSet::new();
    let mut paths = Vec::new();
    for change in &profile.changes {
        let path = LogicalPath::parse(&change.path).map_err(|_| "invalid proposal path")?;
        if change.expected.bytes > 65_536
            || change.object.bytes > 65_536
            || !names.insert(path.clone())
        {
            return Err("duplicate or oversized proposal change".into());
        }
        paths.push(path);
    }
    let base = GitId::parse(&profile.base).map_err(|e| e.to_string())?;
    let repository = Repository::new(
        profile.git_executable,
        profile.repository,
        Limits::default(),
    )
    .map_err(|e| e.to_string())?;
    let mut source = ReadCache::new(profile.byte_source).map_err(|e| e.to_string())?;
    let mut objects = BTreeMap::new();
    let mut guards = Vec::new();
    let mut replacements = Vec::new();
    for (change, path) in profile.changes.iter().zip(paths) {
        if !objects.contains_key(&change.object) {
            let mut guard = source
                .open_verified(&change.object)
                .map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            (&mut guard)
                .take(65_537)
                .read_to_end(&mut bytes)
                .map_err(|_| "cannot read verified proposal bytes")?;
            if bytes.len() > 65_536 || Object::from_bytes(&bytes) != change.object {
                return Err("proposal bytes differ from exact pin".into());
            }
            objects.insert(change.object.clone(), bytes);
            guards.push(guard);
        }
        replacements.push(Replacement {
            path,
            expected: change.expected.clone(),
            bytes: objects[&change.object].clone(),
        });
    }
    let checked = repository
        .check_proposal(base, &replacements, &profile.identity, &profile.message)
        .map_err(|e| e.to_string())?;
    let changes: Vec<_> = checked.changes()
        .map(|(path, expected, object)| json!({"path":path.as_str(),"expected":expected,"object":object}))
        .collect();
    let mut output = json!({
        "schema":"tig.git-proposal-result.experimental-1",
        "action":if create {"create"} else {"check"},
        "base":checked.base().as_str(),"changes":changes,
        "verified_objects":objects.len(),"proposal":null,
        "current_selection":false,"complete_semantic_coverage":false,"payload_custody":false
    });
    if create {
        let proposal = checked.create().map_err(|e| e.to_string())?;
        output["proposal"] =
            json!({"commit":proposal.commit().as_str(),"reference":proposal.reference()});
    }
    writeln!(std::io::stdout().lock(), "{output}").map_err(|_| "cannot write proposal result")?;
    drop(guards);
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
